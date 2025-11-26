<script lang="ts">
  import { onDestroy, onMount } from "svelte";

  type Message = {
    id: string;
    username: string;
    text: string;
    created_at: number;
  };

  const API_BASE = (import.meta.env.VITE_API_BASE ?? "") as string;

  const apiUrl = (path: string) => (API_BASE ? `${API_BASE}${path}` : path);

  let username = $state("");
  let claimed = $state(false);
  let claiming = $state(false);
  let messageText = $state("");
  let messages = $state<Message[]>([]);
  const messageIds = new Set<string>();
  let status = $state<string | null>(null);
  let eventSource = $state<EventSource | null>(null);
  let sessionDuration = $state(0);
  let remainingSeconds = $state<number | null>(null);
  let countdownHandle = $state<ReturnType<typeof setInterval> | null>(null);
  let activeUsers = $state<Set<string>>(new Set());
  let autoReclaimEnabled = $state(true);
  let usersEventSource = $state<EventSource | null>(null);
  let totalMessages = $state(0);
  let statsRefreshHandle: ReturnType<typeof setInterval> | null = null;

  const SETTINGS_KEY = "chat.sacha.house.settings";
  const STATS_REFRESH_INTERVAL_MS = 20000;
  const MAX_MESSAGES = 200;
  const RECLAIM_TRIGGER_SECONDS = 5;

  function loadSettings() {
    try {
      const stored = localStorage.getItem(SETTINGS_KEY);
      if (stored) {
        const settings = JSON.parse(stored);
        if (typeof settings.autoReclaimEnabled === "boolean") {
          autoReclaimEnabled = settings.autoReclaimEnabled;
        }
      }
    } catch (err) {
      // Use defaults
    }
  }

  function saveSettings() {
    try {
      const settings = {
        autoReclaimEnabled,
      };
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    } catch (err) {
      // Ignore
    }
  }

  async function fetchTotalMessages() {
    try {
      const res = await fetch(apiUrl("/api/stats"));
      if (!res.ok) return;
      const data = await res.json();
      if (typeof data?.total_messages === "number") {
        totalMessages = data.total_messages;
      }
    } catch (err) {
      // Silently fail
    }
  }

  function startStatsRefresh() {
    stopStatsRefresh();
    fetchTotalMessages();
    statsRefreshHandle = window.setInterval(() => {
      fetchTotalMessages();
    }, STATS_REFRESH_INTERVAL_MS);
  }

  function stopStatsRefresh() {
    if (statsRefreshHandle !== null) {
      clearInterval(statsRefreshHandle);
      statsRefreshHandle = null;
    }
  }

  onMount(() => {
    loadSettings();
    loadMessages();
    restoreUsername();
    fetchActiveUsers();
    startUsersStream();
    startStatsRefresh();
    return () => {
      stopCountdown();
      stopStatsRefresh();
      eventSource?.close();
      usersEventSource?.close();
    };
  });

  async function claimUsername(event: SubmitEvent) {
    event.preventDefault();
    if (!username.trim()) return;
    claiming = true;
    status = "Claiming username…";
    try {
      const res = await fetch(apiUrl("/api/username/claim"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        credentials: "include",
        body: JSON.stringify({ username: username.trim() }),
      });
      if (!res.ok) {
        const err = await safeJson(res);
        throw new Error(err?.error ?? `claim failed (${res.status})`);
      }
      const data = await res.json();
      sessionDuration = normalizeSeconds(data?.expires_in);
      claimed = true;
      status = "Username locked in.";
      startCountdown(sessionDuration);
      await loadMessages();
      startStream();
      startUsersStream();
    } catch (err) {
      status = err instanceof Error ? err.message : "Claim failed";
      claimed = false;
    } finally {
      claiming = false;
    }
  }

  async function fetchActiveUsers() {
    try {
      const res = await fetch(apiUrl("/api/users"));
      if (!res.ok) return;
      const data = await res.json();
      if (data?.users && Array.isArray(data.users)) {
        activeUsers = new Set(data.users);
      }
    } catch (err) {
      // Silently fail
    }
  }

  function startUsersStream() {
    usersEventSource?.close();
    usersEventSource = new EventSource(apiUrl("/api/users/sse"));
    usersEventSource.addEventListener("user", (event) => {
      try {
        const userEvent = JSON.parse(event.data);
        if (userEvent?.action === "ADD") {
          activeUsers = new Set([...activeUsers, userEvent.username]);
        } else if (userEvent?.action === "REMOVE") {
          const newSet = new Set(activeUsers);
          newSet.delete(userEvent.username);
          activeUsers = newSet;
        }
      } catch (err) {
        console.error("Bad user event", err);
      }
    });
    usersEventSource.addEventListener("error", () => {
      // Reconnect handled by EventSource
    });
  }

  async function restoreUsername() {
    try {
      const res = await fetch(apiUrl("/api/username/current"), {
        credentials: "include",
      });
      if (!res.ok) return;
      const data = await res.json();
      if (data?.username) {
        username = data.username;
        claimed = true;
        const expiresIn = normalizeSeconds(data?.expires_in);
        if (expiresIn > 0) {
          sessionDuration = expiresIn;
          startCountdown(expiresIn);
          startStream();
        }
      }
    } catch (err) {
      // Silently fail - no username to restore
    }
  }

  async function loadMessages() {
    try {
      const res = await fetch(apiUrl("/api/messages?limit=50"));
      if (!res.ok) throw new Error(`fetch messages failed (${res.status})`);
      const data = await res.json();
      messageIds.clear();
      messages = [];
      for (const msg of data?.messages ?? []) {
        insertMessage(msg);
      }
    } catch (err) {
      status = err instanceof Error ? err.message : "Unable to load messages";
    }
  }

  async function sendMessage(event?: Event) {
    event?.preventDefault();
    if (!messageText.trim()) return;
    try {
      const res = await fetch(apiUrl("/api/messages"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        credentials: "include",
        body: JSON.stringify({ text: messageText.trim() }),
      });
      if (!res.ok) {
        const err = await safeJson(res);
        throw new Error(err?.error ?? "Failed to send message");
      }
      messageText = "";
      status = null;
      if (sessionDuration > 0) {
        startCountdown(sessionDuration);
      }
    } catch (err) {
      status = err instanceof Error ? err.message : "Failed to send message";
    }
  }

  async function releaseUsername() {
    try {
      const res = await fetch(apiUrl("/api/username/release"), {
        method: "POST",
        credentials: "include",
      });
      if (!res.ok) {
        const err = await safeJson(res);
        throw new Error(err?.error ?? "Release failed");
      }
      status = "Username released.";
    } catch (err) {
      status = err instanceof Error ? err.message : "Release failed";
    } finally {
      claimed = false;
      sessionDuration = 0;
      stopCountdown();
      eventSource?.close();
    }
  }

  function startStream() {
    eventSource?.close();
    eventSource = new EventSource(apiUrl("/api/messages/sse"), { withCredentials: true });
    eventSource.addEventListener("message", (event) => {
      try {
        const msg: Message = JSON.parse(event.data);
        insertMessage(msg, true);
      } catch (err) {
        console.error("Bad event", err);
      }
    });
    eventSource.addEventListener("error", () => {
      status = "Connection dropped. Reconnecting…";
    });
  }

  function insertMessage(msg: Message, isNew = false) {
    if (messageIds.has(msg.id)) return;
    const next = [...messages];
    let low = 0;
    let high = next.length;
    while (low < high) {
      const mid = (low + high) >> 1;
      if (next[mid].id > msg.id) {
        low = mid + 1;
      } else {
        high = mid;
      }
    }
    next.splice(low, 0, msg);
    messageIds.add(msg.id);
    if (isNew) {
      totalMessages++;
    }
    if (next.length > MAX_MESSAGES) {
      const removed = next.pop();
      if (removed) {
        messageIds.delete(removed.id);
      }
    }
    messages = next;
  }

  async function safeJson(res: Response) {
    try {
      return await res.json();
    } catch {
      return null;
    }
  }

  function handleComposerKey(event: KeyboardEvent) {
    if (event.key === "Enter" && event.ctrlKey) {
      event.preventDefault();
      sendMessage();
    }
  }

  function normalizeSeconds(value: unknown): number {
    const n = Number(value);
    if (!Number.isFinite(n) || n <= 0) return 0;
    return Math.floor(n);
  }

  let autoReclaimTriggered = $state(false);

  function startCountdown(seconds: number) {
    stopCountdown();
    autoReclaimTriggered = false;
    if (seconds <= 0) {
      remainingSeconds = null;
      return;
    }
    sessionDuration = seconds;
    remainingSeconds = seconds;
    countdownHandle = window.setInterval(() => {
      if (remainingSeconds === null) return;
      remainingSeconds = Math.max(0, remainingSeconds - 1);
      if (remainingSeconds === RECLAIM_TRIGGER_SECONDS && autoReclaimEnabled && claimed && !autoReclaimTriggered) {
        autoReclaimTriggered = true;
        claimUsername(new Event("submit") as SubmitEvent);
      }
      if (remainingSeconds === 0) {
        stopCountdown();
        claimed = false;
        sessionDuration = 0;
        eventSource?.close();
        status = "Username auto released.";
      }
    }, 1000);
  }

  function stopCountdown() {
    if (countdownHandle !== null) {
      clearInterval(countdownHandle);
      countdownHandle = null;
    }
    remainingSeconds = null;
  }

  onDestroy(() => {
    stopCountdown();
    eventSource?.close();
  });
</script>

<main class="app">
  <section class="panel">
    <h1 class="title">
      <marquee class="marquee" direction="left">GL0BALLY_AVAILA8LE_CH4T_R00M </marquee>
    </h1>
    <form class="claim" onsubmit={claimUsername}>
      <label>
        Username
        <input placeholder="pick something short" bind:value={username} disabled={claiming || claimed} />
      </label>
      {#if !claimed}
        <button type="submit" disabled={claiming || !username.trim()}>
          {claiming ? "Claiming…" : "Claim username"}
        </button>
      {:else}
        <div class="input-group">
          <button type="button" onclick={releaseUsername}>
            {autoReclaimEnabled ? "Auto-reclaim" : "Auto-release"}
            {#if remainingSeconds !== null}
              (auto {autoReclaimEnabled ? "reclaim" : "release"} in {remainingSeconds}s){/if}
          </button>
          <label class="toggle">
            <input type="checkbox" bind:checked={autoReclaimEnabled} onchange={saveSettings} />
            <span class="toggle__slider"></span>
            <span class="toggle__label">Auto-reclaim</span>
          </label>
        </div>
        <p class="claimed">✅ {username} locked for this session.</p>
      {/if}
    </form>

    <form class="composer" onsubmit={sendMessage}>
      <textarea
        placeholder={claimed ? "Say something nice" : "Claim a username first"}
        bind:value={messageText}
        maxlength={240}
        disabled={!claimed}
        onkeydown={handleComposerKey}
      ></textarea>
      <div class="composer__meta">
        <span>{messageText.trim().length}/240</span>
        <button type="submit" disabled={!claimed || !messageText.trim()}>Send</button>
      </div>
    </form>

    {#if status}
      <p class="status">{status}</p>
    {/if}
  </section>

  <section class="history">
    <header class="history__header">
      <h2>Messages</h2>
      <button type="button" onclick={loadMessages}>Refresh</button>
    </header>
    {#if messages.length === 0}
      <p class="empty">No messages yet.</p>
    {:else}
      <ul>
        {#each messages as message (message.id)}
          <li>
            <header>
              <strong>
                {message.username}
                <span class="badge" class:active={activeUsers.has(message.username)}></span>
              </strong>
              <time>{new Date(message.created_at * 1000).toLocaleTimeString()}</time>
            </header>
            <p>{message.text}</p>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</main>
