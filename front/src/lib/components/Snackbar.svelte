<script lang="ts">
  import { slide } from "svelte/transition";
  import { notifications } from "../stores/notifications";
  import type { Notification } from "../types";

  interface Props {
    position?: "top" | "bottom";
  }

  let { position = "bottom" }: Props = $props();

  const SNACKBAR_DURATION_MS = 5000;

  let notificationList = $state<Notification[]>([]);
  let remainingSeconds = $state<Map<string, number>>(new Map());

  $effect(() => {
    const unsubscribe = notifications.subscribe((state) => {
      notificationList = state.notifications;
    });
    return unsubscribe;
  });

  $effect(() => {
    const interval = setInterval(() => {
      const now = Date.now();
      const newRemaining = new Map<string, number>();
      for (const notification of notificationList) {
        const elapsed = now - notification.createdAt;
        const remaining = Math.max(0, Math.ceil((SNACKBAR_DURATION_MS - elapsed) / 1000));
        newRemaining.set(notification.id, remaining);
      }
      remainingSeconds = newRemaining;
    }, 100);
    return () => clearInterval(interval);
  });

  function handleDismiss(id: string) {
    notifications.dismiss(id);
  }
</script>

<div class="snackbar-container" class:top={position === "top"} class:bottom={position === "bottom"}>
  {#each notificationList as notification (notification.id)}
    <button
      type="button"
      class="snackbar"
      class:snackbar--error={notification.type === "error"}
      class:snackbar--success={notification.type === "success"}
      class:snackbar--info={notification.type === "info"}
      onclick={() => handleDismiss(notification.id)}
      transition:slide={{ axis: "y", duration: 200 }}
    >
      <div class="snackbar__content">
        <div class="snackbar__message">{notification.message}</div>
        {#if remainingSeconds.has(notification.id)}
          <div class="snackbar__countdown">
            (hides in {remainingSeconds.get(notification.id)}s, click to dismiss)
          </div>
        {/if}
      </div>
    </button>
  {/each}
</div>

<style>
  .snackbar-container {
    position: fixed;
    left: 0;
    right: 0;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    pointer-events: none;
    z-index: 1000;
    max-width: 960px;
    margin: 0 auto;
  }

  .snackbar-container.bottom {
    bottom: 0;
    flex-direction: column-reverse;
    padding: 0.5rem;
  }

  .snackbar-container.top {
    top: 0;
    flex-direction: column;
    padding: 0.5rem;
  }

  @media (min-width: 768px) {
    .snackbar-container {
      padding: 1rem;
    }

  }

  .snackbar {
    background: #1a1a1a;
    border: 1px solid #2a2a2a;
    color: #e0e0e0;
    padding: 0.75rem 1rem;
    border-radius: 0;
    font-size: 0.9rem;
    cursor: pointer;
    pointer-events: auto;
    width: 100%;
    text-align: center;
    transition:
      background 0.15s,
      border-color 0.15s;
  }

  .snackbar__content {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .snackbar__message {
    font-size: 0.9rem;
  }

  .snackbar__countdown {
    font-size: 0.7rem;
    opacity: 0.5;
    color: inherit;
  }

  .snackbar:hover {
    background: #222;
    border-color: #3a3a3a;
  }

  .snackbar--error {
    background: #2a1111;
    border-color: #4a2222;
    color: #ff4444;
  }

  .snackbar--error:hover {
    background: #3a1a1a;
    border-color: #5a3333;
  }

  .snackbar--success {
    background: #112a11;
    border-color: #224a22;
    color: #44ff44;
  }

  .snackbar--success:hover {
    background: #1a3a1a;
    border-color: #335a33;
  }

  .snackbar--info {
    background: #111a2a;
    border-color: #222a4a;
    color: #4488ff;
  }

  .snackbar--info:hover {
    background: #1a2a3a;
    border-color: #333a5a;
  }
</style>
