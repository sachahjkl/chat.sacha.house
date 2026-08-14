import type {
  ClaimResponse,
  CurrentUsernameResponse,
  ErrorResponse,
  IntervalHandle,
  Julid,
  Message,
  MessagesResponse,
  NotificationType,
  PostMessageResponse,
  StatsResponse,
  UserEvent,
  Username,
  UsernameReleaseResponse,
  UsersResponse,
} from "./types";
import { normalizeSeconds, safeJson, safeJsonAsync } from "./utils";

import { SvelteSet } from "svelte/reactivity";
import { TopKStore } from "./TopKStore.svelte";
import { notifications } from "./stores/notifications";

const STORAGE_KEY = "chat.sacha.house.settings";
const STATS_REFRESH_INTERVAL_MS = 20000;
const MAX_MESSAGES = 200;
const RECLAIM_TRIGGER_SECONDS = 5;

export class ChatSession {
  apiBase: string;
  #_sessionId: Julid | null = null;

  claimedUsername = $state<Username | null>("");
  claimed = $derived(this.claimedUsername !== null && this.claimedUsername !== "");
  #messageList: TopKStore<string, Message>;
  messages: Message[] = [];
  remainingSeconds = $state<number | null>(null);
  activeUsernames = new SvelteSet<Username>();
  totalMessages = $state(0);

  #eventSource: EventSource | null = null;
  #usersEventSource: EventSource | null = null;
  #countdownHandle: IntervalHandle | null = null;
  #statsRefreshHandle: IntervalHandle | null = null;
  #autoReclaimTriggered = false;
  #autoReclaimEnabled = $state(false);
  #notify: (message: string, type: NotificationType) => void;

  #abortController: AbortController = new AbortController();

  constructor(apiBase: string, notifyFn: (message: string, type: NotificationType) => void) {
    this.apiBase = apiBase;
    this.#autoReclaimEnabled = localStorage.getItem(STORAGE_KEY + ".autoReclaimEnabled") === "true";
    this.#_sessionId = sessionStorage.getItem(STORAGE_KEY + ".sessionId");
    this.#notify = notifyFn;
    this.#messageList = new TopKStore<string, Message>(MAX_MESSAGES, (msg) => msg.id);
    this.messages = this.#messageList.items;
  }

  private apiUrl(path: string): string {
    return this.apiBase ? `${this.apiBase}${path}` : path;
  }

  private parseUsername(username: string): Username {
    return username.trim().toLowerCase();
  }

  async claimUsername({
    newUsername,
    silent = false,
  }: {
    newUsername: string;
    silent?: boolean;
  }): Promise<{ success: boolean }> {
    this.#abortController.abort();
    this.#abortController = new AbortController();

    const username = this.parseUsername(newUsername);
    if (!username) return { success: false };

    if (!silent) {
      notifications.showNotification("Claiming username…", "info");
    }

    try {
      const headers: Record<string, string> = {
        "Content-Type": "application/json",
      };

      if (this.sessionId) {
        headers.Authorization = `Bearer ${this.sessionId}`;
      }

      const res = await fetch(this.apiUrl("/api/username/claim"), {
        method: "POST",
        headers,
        body: JSON.stringify({ username }),
        signal: this.#abortController.signal,
      });

      if (!res.ok) {
        const err = await safeJsonAsync<ErrorResponse>(res);
        throw new Error(err?.error ?? `claim failed (${res.status})`);
      }

      const data = await safeJsonAsync<ClaimResponse>(res);
      if (!data) {
        throw new Error("Invalid claim response");
      }

      const sessionDuration = normalizeSeconds(data.expires_in);
      this.claimedUsername = data.username;
      this.sessionId = data.session_id;

      this.startCountdown(sessionDuration);

      return { success: true };
    } catch (err) {
      this.clearSession();
      if (!silent) {
        this.#notify(err instanceof Error ? err.message : "Claim failed", "error");
      }
      return { success: false };
    } finally {
    }
  }

  async releaseUsername(): Promise<{ success: boolean }> {
    this.#abortController.abort();
    this.#abortController = new AbortController();

    try {
      const headers: Record<string, string> = {};
      if (this.sessionId) {
        headers.Authorization = `Bearer ${this.sessionId}`;
      }
      const res = await fetch(this.apiUrl("/api/username/release"), {
        method: "POST",
        headers,
        signal: this.#abortController.signal,
      });

      if (!res.ok) {
        const err = await safeJsonAsync<ErrorResponse>(res);
        throw new Error(err?.error ?? "Release failed");
      }

      const data = await safeJsonAsync<UsernameReleaseResponse>(res);
      if (!data) {
        throw new Error("Invalid username release response");
      }

      if (data.success) {
        this.clearSession();
        this.#notify("Username released.", "success");
        return { success: true };
      } else {
        return { success: false };
      }
    } catch (err) {
      this.#notify(err instanceof Error ? err.message : "Release failed", "error");
      return { success: false };
    } finally {
      this.stopCountdown();
    }
  }

  async tryRestoreUsername(): Promise<void> {
    try {
      const headers: Record<string, string> = {};

      if (this.sessionId) {
        headers.Authorization = `Bearer ${this.sessionId}`;
      }

      const res = await fetch(this.apiUrl("/api/username/current"), {
        headers,
      });
      if (!res.ok) {
        this.clearSession();
        return;
      }

      const data = await safeJsonAsync<CurrentUsernameResponse>(res);
      if (!data) {
        this.clearSession();
        return;
      }

      if (!data.username) {
        this.clearSession();
        return;
      }

      const expiresIn = normalizeSeconds(data.expires_in);

      if (!expiresIn) {
        this.clearSession();
        return;
      }

      this.claimedUsername = data.username;

      this.startCountdown(expiresIn);
    } catch {
      // Silently fail - no username to restore
    }
  }

  async loadMessages(): Promise<void> {
    try {
      const res = await fetch(this.apiUrl("/api/messages?limit=50"));
      if (!res.ok) throw new Error(`fetch messages failed (${res.status})`);
      const data = await safeJsonAsync<MessagesResponse>(res);

      if (!data) {
        this.#messageList.clear();
        return;
      }

      this.#messageList.load(data.messages);
    } catch (err) {
      notifications.showNotification(
        err instanceof Error ? err.message : "Unable to load messages",
        "error",
      );
    }
  }

  async sendMessage(text: string): Promise<{ success: boolean }> {
    const trimmedText = text.trim();
    if (!trimmedText) return { success: false };
    try {
      const headers: Record<string, string> = { "Content-Type": "application/json" };

      if (this.sessionId) {
        headers.Authorization = `Bearer ${this.sessionId}`;
      }

      const res = await fetch(this.apiUrl("/api/messages"), {
        method: "POST",
        headers,
        body: JSON.stringify({ text: trimmedText }),
      });

      if (!res.ok) {
        const err = await safeJsonAsync<ErrorResponse>(res);
        throw new Error(err?.error ?? "Failed to send message");
      }

      const data = await safeJsonAsync<PostMessageResponse>(res);
      if (!data || !data.success) {
        throw new Error("Invalid send message response");
      }

      return { success: true };
    } catch (err) {
      notifications.showNotification(
        err instanceof Error ? err.message : "Failed to send message",
        "error",
      );
      return { success: false };
    }
  }

  private insertMessage({ msg, isNew = false }: { msg: Message; isNew?: boolean }) {
    const inserted = this.#messageList.insert(msg);
    if (inserted && isNew) {
      this.totalMessages++;
    }
  }

  async fetchActiveUsers(): Promise<void> {
    try {
      const res = await fetch(this.apiUrl("/api/users"));

      if (!res.ok) return;

      const data = await safeJsonAsync<UsersResponse>(res);
      if (!data) {
        throw new Error("Invalid users response");
      }

      this.activeUsernames.clear();

      for (const user of data.users) {
        this.activeUsernames.add(user);
      }
    } catch (err) {
      console.error("Failed to fetch active users", err);
    }
  }

  async fetchTotalMessages(): Promise<void> {
    try {
      const res = await fetch(this.apiUrl("/api/stats"));

      if (!res.ok) return;

      const data = await safeJsonAsync<StatsResponse>(res);
      if (!data) {
        return;
      }

      this.totalMessages = data.total_messages;
    } catch {
      // Silently fail
    }
  }

  private startMessagesStream() {
    this.#eventSource?.close();
    this.#eventSource = new EventSource(this.apiUrl("/api/messages/sse"), {
      withCredentials: true,
    });
    this.#eventSource.addEventListener("message", (event) => {
      try {
        const msg = safeJson<Message>(event.data);
        if (!msg) {
          throw new Error("Invalid message");
        }
        this.insertMessage({ msg, isNew: true });
      } catch (err) {
        console.error("Bad event", err);
      }
    });
    this.#eventSource.addEventListener("error", () => {
      this.#notify("Connection dropped. Reconnecting…", "info");
    });
  }

  private startUsersStream() {
    this.#usersEventSource?.close();
    this.#usersEventSource = new EventSource(this.apiUrl("/api/users/sse"));

    this.#usersEventSource.addEventListener("user", (event) => {
      try {
        const userEvent = safeJson<UserEvent>(event.data);
        if (!userEvent) {
          throw new Error("Invalid user event");
        }

        if (userEvent.action === "ADD") {
          this.activeUsernames.add(userEvent.username);
        } else if (userEvent.action === "REMOVE") {
          this.activeUsernames.delete(userEvent.username);

          if (userEvent.username === this.claimedUsername) {
            this.clearSession();
            notifications.showNotification("Username released.", "info");
          }
        }
      } catch (err) {
        console.error("Bad user event", err);
      }
    });
    this.#usersEventSource.addEventListener("error", () => {
      // Reconnect handled by EventSource
    });
  }

  mount() {
    this.loadMessages();
    this.tryRestoreUsername();
    this.fetchActiveUsers();
    this.startEventSources();
    this.startStatsPolling();
    return () => this.destroy();
  }

  startEventSources() {
    this.startMessagesStream();
    this.startUsersStream();
  }

  private closeEventSources() {
    this.#eventSource?.close();
    this.#eventSource = null;
    this.#usersEventSource?.close();
    this.#usersEventSource = null;
  }

  private startCountdown(seconds: number) {
    this.stopCountdown();
    this.#autoReclaimTriggered = false;
    if (seconds <= 0) {
      this.remainingSeconds = null;
      return;
    }
    this.remainingSeconds = seconds;
    this.#countdownHandle = setInterval(() => {
      if (this.remainingSeconds === null) return;

      this.remainingSeconds = Math.max(0, this.remainingSeconds - 1);

      if (
        this.remainingSeconds === RECLAIM_TRIGGER_SECONDS &&
        this.#autoReclaimEnabled &&
        this.claimed &&
        !this.#autoReclaimTriggered
      ) {
        this.#autoReclaimTriggered = true;
        if (this.claimedUsername) {
          this.claimUsername({ newUsername: this.claimedUsername, silent: true });
        }
      }

      if (this.remainingSeconds === 0) {
        this.stopCountdown();
        this.claimedUsername = null;
        this.releaseUsername();
        notifications.showNotification("Username auto released.", "info");
      }
    }, 1000);
  }

  get autoReclaimEnabled(): boolean {
    return this.#autoReclaimEnabled;
  }

  set autoReclaimEnabled(value: boolean) {
    this.#autoReclaimEnabled = value;
    localStorage.setItem(STORAGE_KEY + ".autoReclaimEnabled", value.toString());
  }

  private stopCountdown() {
    if (this.#countdownHandle !== null) {
      clearInterval(this.#countdownHandle);
      this.#countdownHandle = null;
    }
    this.remainingSeconds = null;
  }

  startStatsPolling() {
    this.stopStatsRefresh();
    this.fetchTotalMessages();
    this.#statsRefreshHandle = setInterval(() => {
      this.fetchTotalMessages();
    }, STATS_REFRESH_INTERVAL_MS);
  }

  private stopStatsRefresh() {
    if (this.#statsRefreshHandle !== null) {
      clearInterval(this.#statsRefreshHandle);
      this.#statsRefreshHandle = null;
    }
  }

  private clearSession() {
    this.sessionId = null;
    this.claimedUsername = null;
  }

  get sessionId(): Julid | null {
    return this.#_sessionId;
  }

  set sessionId(value: Julid | null) {
    this.#_sessionId = value;
    if (value) {
      sessionStorage.setItem(STORAGE_KEY + ".sessionId", value);
    } else {
      sessionStorage.removeItem(STORAGE_KEY + ".sessionId");
    }
  }

  destroy() {
    this.stopCountdown();
    this.stopStatsRefresh();
    this.closeEventSources();
  }
}
