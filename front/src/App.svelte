<script lang="ts">
  import { ScrollState } from "runed";
  import { onDestroy, onMount } from "svelte";
  import Composer from "./lib/components/Composer.svelte";
  import MessageList from "./lib/components/MessageList.svelte";
  import Snackbar from "./lib/components/Snackbar.svelte";
  import UsernameClaim from "./lib/components/UsernameClaim.svelte";
  import { notifications } from "./lib/stores/notifications";
  import type { Message } from "./lib/types";

  const API_BASE = (import.meta.env.VITE_API_BASE ?? "") as string;

  const apiUrl = (path: string) => (API_BASE ? `${API_BASE}${path}` : path);

  let username = $state("");
  let claimed = $state(false);
  let claiming = $state(false);
  let messageText = $state("");
  let messages = $state<Message[]>([]);
  const messageIds = new Set<string>();
  let eventSource = $state<EventSource | null>(null);
  let sessionDuration = $state(0);
  let remainingSeconds = $state<number | null>(null);
  let countdownHandle = $state<ReturnType<typeof setInterval> | null>(null);
  let activeUsers = $state<Set<string>>(new Set());
  let autoReclaimEnabled = $state(true);
  let usersEventSource = $state<EventSource | null>(null);
  let totalMessages = $state(0);
  let statsRefreshHandle: ReturnType<typeof setInterval> | null = null;
  let windowElement = $state<HTMLElement>();
  let composerPanel = $state<HTMLElement>();
  let viewportOffset = $state(0);
  const scroll = new ScrollState({
    element: () => window,
  });

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

    function handleViewportChange() {
      if (!window.visualViewport) return;
      const viewport = window.visualViewport;
      const keyboardHeight = window.innerHeight - viewport.height;
      viewportOffset = keyboardHeight > 0 ? -keyboardHeight : 0;
    }

    if (window.visualViewport) {
      window.visualViewport.addEventListener("resize", handleViewportChange);
      window.visualViewport.addEventListener("scroll", handleViewportChange);
      handleViewportChange();
    }

    return () => {
      stopCountdown();
      stopStatsRefresh();
      eventSource?.close();
      usersEventSource?.close();
      if (window.visualViewport) {
        window.visualViewport.removeEventListener("resize", handleViewportChange);
        window.visualViewport.removeEventListener("scroll", handleViewportChange);
      }
    };
  });

  async function handleClaim({ usernameInput, silent = false }: { usernameInput: string; silent?: boolean }) {
    if (!usernameInput.trim()) return;
    claiming = true;
    if (!silent) {
      notifications.showNotification("Claiming username…", "info");
    }
    try {
      const res = await fetch(apiUrl("/api/username/claim"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        credentials: "include",
        body: JSON.stringify({ username: usernameInput.trim() }),
      });
      if (!res.ok) {
        const err = await safeJson(res);
        throw new Error(err?.error ?? `claim failed (${res.status})`);
      }
      const data = await res.json();
      sessionDuration = normalizeSeconds(data?.expires_in);
      claimed = true;
      username = usernameInput.trim();
      startCountdown(sessionDuration);
      await loadMessages();
      startStream();
      startUsersStream();
    } catch (err) {
      if (!silent) {
        notifications.showNotification(err instanceof Error ? err.message : "Claim failed", "error");
      }
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
      notifications.showNotification(err instanceof Error ? err.message : "Unable to load messages", "error");
    }
  }

  async function handleSendMessage(text: string) {
    if (!text.trim()) return;
    try {
      const res = await fetch(apiUrl("/api/messages"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        credentials: "include",
        body: JSON.stringify({ text: text.trim() }),
      });
      if (!res.ok) {
        const err = await safeJson(res);
        throw new Error(err?.error ?? "Failed to send message");
      }
      if (sessionDuration > 0) {
        startCountdown(sessionDuration);
      }
    } catch (err) {
      notifications.showNotification(err instanceof Error ? err.message : "Failed to send message", "error");
    }
  }

  async function handleRelease() {
    try {
      const res = await fetch(apiUrl("/api/username/release"), {
        method: "POST",
        credentials: "include",
      });
      if (!res.ok) {
        const err = await safeJson(res);
        throw new Error(err?.error ?? "Release failed");
      }
      notifications.showNotification("Username released.", "success");
    } catch (err) {
      notifications.showNotification(err instanceof Error ? err.message : "Release failed", "error");
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
      notifications.showNotification("Connection dropped. Reconnecting…", "info");
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

  function handleToggleAutoReclaim(enabled: boolean) {
    autoReclaimEnabled = enabled;
    saveSettings();
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
        handleClaim({ usernameInput: username, silent: true });
      }
      if (remainingSeconds === 0) {
        stopCountdown();
        claimed = false;
        sessionDuration = 0;
        eventSource?.close();
        notifications.showNotification("Username auto released.", "info");
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

  function scrollToTop() {
    scroll.scrollToTop();
  }

  onDestroy(() => {
    stopCountdown();
    eventSource?.close();
  });
</script>

<div class="app">
  <header class="header">
    <h1 class="title">
      <!-- svelte-ignore a11y_distracting_elements -->
      <marquee class="marquee" direction="left">GL0BALLY_AVAILA8LE_CH4T_R00M </marquee>
    </h1>
  </header>

  <main class="main">
    <section class="panel">
      <UsernameClaim
        bind:username
        {claimed}
        {claiming}
        bind:autoReclaimEnabled
        {remainingSeconds}
        onClaim={(usernameInput) => handleClaim({ usernameInput })}
        onRelease={handleRelease}
        onToggleAutoReclaim={handleToggleAutoReclaim}
      />
    </section>

    <section class="history">
      <header class="history__header">
        <span class="history__count">{totalMessages} MESSAGES</span>
        <button
          class="refresh-button"
          aria-label="Refresh messages"
          title="Refresh messages"
          type="button"
          onclick={loadMessages}>Refresh</button
        >
      </header>
      <MessageList {messages} {activeUsers} />
    </section>

    <section
      class="panel"
      bind:this={composerPanel}
      style:transform={viewportOffset !== 0 ? `translateY(${viewportOffset}px)` : undefined}
    >
      <Composer
        {claimed}
        bind:messageText
        onSubmit={handleSendMessage}
        showScrollToTop={scroll.y > 0}
        onScrollToTop={scrollToTop}
      />
    </section>
  </main>
  <Snackbar position="bottom" />
</div>

<style>
  .app {
    margin: 0 auto;
    max-width: 960px;
    padding: 0.5rem;
  }
  @media (min-width: 768px) {
    .app {
      padding: 1rem;
    }
  }

  .header {
    margin-bottom: 0.5rem;
  }

  .main {
    display: flex;
    flex-direction: column;
    min-height: 100vh;
    gap: 0.75rem;
  }

  section {
    background: #111;
    border: 1px solid #2a2a2a;
    border-radius: 0;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .panel {
    display: flex;
    flex-direction: column;
    gap: 10px;
    position: sticky;
    bottom: 0;
    transition: transform 0.2s ease-out;
  }

  .history {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    flex: 1;
    overflow-y: auto;
  }

  .history__header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .history__count {
    font-size: 0.85rem;
    color: #aaa;
    font-weight: 600;
  }

  .history__header button {
    font-size: 0.85rem;
    padding: 0.5rem 1rem;
  }

  .title {
    background: #0a0a0a;
    color: #44ff44;
    padding: 0.25rem 0;
    border-radius: 0;
    font-size: 1.25rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.15em;
    margin: 0;
    border: 1px solid #224a22;
    text-shadow: 0 0 8px rgba(68, 255, 68, 0.3);
    overflow: hidden;
    white-space: nowrap;
    display: flex;
    align-items: center;
    flex-shrink: 0;
  }

  @media (min-width: 768px) {
    .title {
      font-size: 1.5rem;
    }
  }

  .title .marquee {
    display: inline-block;
  }

  .refresh-button {
    align-self: flex-start;
    border-radius: 0;
    border: 1px solid #2a2a2a;
    padding: 0.625rem 1.25rem;
    font: inherit;
    background: #1a1a1a;
    color: #e0e0e0;
    cursor: pointer;
    transition:
      background 0.15s,
      border-color 0.15s;
  }

  .refresh-button:hover:not(:disabled) {
    background: #222;
    border-color: #3a3a3a;
  }

  .refresh-button:active:not(:disabled) {
    background: #0f0f0f;
  }

  .refresh-button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>
