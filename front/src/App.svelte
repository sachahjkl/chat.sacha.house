<script lang="ts">
  import { ScrollState } from "runed";
  import { onMount } from "svelte";
  import { innerHeight } from "svelte/reactivity/window";
  import { visualViewportHeight, withCssVariables } from "./lib/browser.svelte";
  import { ChatSession } from "./lib/ChatSession.svelte";
  import Composer from "./lib/components/Composer.svelte";
  import MessageList from "./lib/components/MessageList.svelte";
  import ToastStack from "./lib/components/ToastStack.svelte";
  import UsernameClaim from "./lib/components/UsernameClaim.svelte";
  import { notifications } from "./lib/stores/notifications";

  const API_BASE = (import.meta.env.VITE_API_BASE ?? "") as string;

  const session = new ChatSession(API_BASE, notifications.showNotification);

  let usernameInput = $state<string>("");
  let messageInput = $state("");

  let composerElement: Composer | null = null;

  let viewportOffset = $derived.by(() => {
    if (!visualViewportHeight.current || !innerHeight.current) return 0;
    const keyboardHeight = innerHeight.current - visualViewportHeight.current;
    return keyboardHeight > 0 ? -keyboardHeight : 0;
  });

  let claiming = $state(false);

  const scroll = new ScrollState({
    element: () => window,
  });

  onMount(() => session.mount());

  function onClaim(usernameInput: string) {
    claiming = true;
    session
      .claimUsername({ newUsername: usernameInput })
      .then((result) => {
        if (result.success) {
          usernameInput = "";
          composerElement?.focusTextarea();
        }
      })
      .finally(() => {
        claiming = false;
      });
  }

  function onRelease() {
    session.releaseUsername().then((result) => {
      if (result.success) {
        usernameInput = "";
      }
    });
  }

  function onToggleAutoReclaim(enabled: boolean) {
    session.autoReclaimEnabled = enabled;
  }
</script>

<div class="app">
  <header class="header">
    <h1 class="title">
      <!-- svelte-ignore a11y_distracting_elements -->
      <marquee class="marquee" direction="left">GL0BALLY_AVAILA8LE_CH4T_R00M </marquee>
    </h1>
  </header>

  <main class="main">
    <section class="panel username-claim">
      <UsernameClaim
        claimedUsername={session.claimedUsername}
        {claiming}
        bind:usernameInput
        bind:autoReclaimEnabled={session.autoReclaimEnabled}
        remainingSeconds={session.remainingSeconds}
        {onClaim}
        {onRelease}
        {onToggleAutoReclaim}
      />
    </section>

    <section class="history">
      <header class="history__header">
        <span class="history__count">{session.totalMessages} MESSAGES</span>
        <button
          class="refresh-button"
          aria-label="Refresh messages"
          title="Refresh messages"
          type="button"
          onclick={() => session.loadMessages()}>Refresh</button
        >
      </header>
      <MessageList messages={session.messages} activeUsernames={session.activeUsernames} />
    </section>
    <div class="composer-container">
      <section
        class="panel composer"
        class:keyboard-mode={viewportOffset !== 0}
        {@attach withCssVariables({ "keyboard-height": `${viewportOffset}px` })}
      >
        <Composer
          enabled={session.claimed}
          bind:messageText={messageInput}
          onSubmit={(text) => session.sendMessage(text)}
          showScrollToTop={scroll.y > 0}
          onScrollToTop={() => scroll.scrollToTop()}
          bind:this={composerElement}
        />
      </section>
    </div>
  </main>
  <!-- <Snackbar position={composerFocused ? "top" : "bottom"} /> -->
  <ToastStack />
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

  .composer-container {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    margin: 0 auto;
    max-width: 960px;
    padding-inline: 0.5rem;
  }

  @media (min-width: 768px) {
    .composer-container {
      padding-inline: 1rem;
    }
  }

  .composer {
    transition: transform 0.2s ease-out;
  }

  .composer.keyboard-mode {
    transform: translateY(var(--keyboard-height));
  }

  .header {
    margin-bottom: 0.5rem;
  }

  .main {
    display: flex;
    position: relative;
    flex-direction: column;
    min-height: 100vh;
    gap: 0.75rem;
    margin-bottom: 300px;
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

  .username-claim {
    display: flex;
    flex-direction: column;
    gap: 10px;
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
