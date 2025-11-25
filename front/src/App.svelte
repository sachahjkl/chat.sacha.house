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

  onMount(() => {
    loadMessages();
    restoreUsername();
    return () => {
      stopCountdown();
      eventSource?.close();
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
    } catch (err) {
      status = err instanceof Error ? err.message : "Claim failed";
      claimed = false;
    } finally {
      claiming = false;
    }
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
    eventSource = new EventSource(apiUrl("/api/sse"), { withCredentials: true });
    eventSource.addEventListener("message", (event) => {
      try {
        const msg: Message = JSON.parse(event.data);
        insertMessage(msg);
      } catch (err) {
        console.error("Bad event", err);
      }
    });
    eventSource.addEventListener("error", () => {
      status = "Connection dropped. Reconnecting…";
    });
  }

  function insertMessage(msg: Message) {
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
    if (next.length > 200) {
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

  function startCountdown(seconds: number) {
    stopCountdown();
    if (seconds <= 0) {
      remainingSeconds = null;
      return;
    }
    sessionDuration = seconds;
    remainingSeconds = seconds;
    countdownHandle = window.setInterval(() => {
      if (remainingSeconds === null) return;
      remainingSeconds = Math.max(0, remainingSeconds - 1);
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
    <h1 class="title">GL0BALLY_AVAILA8LE_CH4T_R00M</h1>
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
        <button type="button" onclick={releaseUsername}>
          Release username
          {#if remainingSeconds !== null}
            (auto release in {remainingSeconds}s){/if}
        </button>
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
              <strong>{message.username}</strong>
              <time>{new Date(message.created_at * 1000).toLocaleTimeString()}</time>
            </header>
            <p>{message.text}</p>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</main>
