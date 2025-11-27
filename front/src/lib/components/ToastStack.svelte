<script lang="ts">
  import { fly } from "svelte/transition";
  import { notifications } from "../stores/notifications";
  import type { Notification } from "../types";

  const TOAST_DURATION_MS = 5000;

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
      const next = new Map<string, number>();
      for (const toast of notificationList) {
        const elapsed = now - toast.createdAt;
        const remaining = Math.max(0, Math.ceil((TOAST_DURATION_MS - elapsed) / 1000));
        next.set(toast.id, remaining);
      }
      remainingSeconds = next;
    }, 100);
    return () => clearInterval(interval);
  });

  function dismiss(id: string) {
    notifications.dismiss(id);
  }
</script>

<div class="toast-stack">
  {#each notificationList as toast (toast.id)}
    <button
      type="button"
      class="toast"
      class:toast--error={toast.type === "error"}
      class:toast--success={toast.type === "success"}
      class:toast--info={toast.type === "info"}
      onclick={() => dismiss(toast.id)}
      transition:fly={{ x: 20, duration: 200 }}
    >
      <div class="toast__content">
        <div class="toast__message">{toast.message}</div>
        {#if remainingSeconds.has(toast.id)}
          <div class="toast__countdown">(hides in {remainingSeconds.get(toast.id)}s)</div>
        {/if}
      </div>
    </button>
  {/each}
</div>

<style>
  .toast-stack {
    position: fixed;
    top: 1rem;
    right: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    pointer-events: none;
    z-index: 1100;
  }

  .toast {
    width: min(70vw, 300px);
    background: #0e0a0a;
    border: 1px solid #2a2a2a;
    color: #e0e0e0;
    padding: 0.75rem 1rem;
    border-radius: 0;
    text-align: left;
    cursor: pointer;
    pointer-events: auto;
    transition:
      background 0.15s,
      border-color 0.15s;
  }

  .toast:hover {
    background: #222;
    border-color: #3a3a3a;
  }

  .toast__content {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .toast__message {
    font-size: 0.9rem;
  }

  .toast__countdown {
    font-size: 0.7rem;
    opacity: 0.5;
  }

  .toast--error {
    background: #2a1111;
    border-color: #4a2222;
    color: #ff4444;
  }

  .toast--error:hover {
    background: #3a1a1a;
    border-color: #5a3333;
  }

  .toast--success {
    background: #112a11;
    border-color: #224a22;
    color: #44ff44;
  }

  .toast--success:hover {
    background: #1a3a1a;
    border-color: #335a33;
  }

  .toast--info {
    background: #111a2a;
    border-color: #222a4a;
    color: #4488ff;
  }

  .toast--info:hover {
    background: #1a2a3a;
    border-color: #333a5a;
  }
</style>
