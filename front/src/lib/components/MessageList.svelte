<script lang="ts">
  import type { SvelteSet } from "svelte/reactivity";
  import { fly } from "svelte/transition";
  import type { Message } from "../types";

  interface Props {
    messages: Message[];
    activeUsers: SvelteSet<string>;
  }

  let { messages, activeUsers }: Props = $props();
</script>

{#if messages.length === 0}
  <p class="empty">No messages yet.</p>
{:else}
  <ul class="message-list">
    {#each messages as message (message.id)}
      <li transition:fly={{ y: -20, duration: 100 }}>
        <header>
          <strong class="username">
            <span class="badge" class:active={activeUsers.has(message.username)}></span>
            {message.username}
          </strong>
          <time>{new Date(message.created_at * 1000).toLocaleTimeString()}</time>
        </header>
        <p>{message.text}</p>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .message-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .message-list li {
    border: 1px solid #2a2a2a;
    border-radius: 0;
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
    background: #0a0a0a;
    transition:
      border-color 0.15s,
      background 0.15s;
  }

  .message-list li:hover {
    border-color: #3a3a3a;
    background: #0f0f0f;
  }

  .message-list li header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.85rem;
    color: #aaa;
    padding-bottom: 0.25rem;
    border-bottom: 1px solid #1a1a1a;
  }

  .username {
    color: #fff;
    font-weight: 600;
  }

  .message-list li time {
    color: #666;
    font-size: 0.8rem;
  }

  .message-list li p {
    margin: 0;
    color: #d0d0d0;
    line-height: 1.6;
  }

  .empty {
    margin: 0;
    color: #666;
    font-style: italic;
    text-align: center;
    padding: 2rem;
  }

  .badge {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #ff4444;
    box-shadow:
      0 0 4px rgba(255, 68, 68, 0.6),
      0 0 8px rgba(255, 68, 68, 0.4);
  }

  .badge.active {
    background: #44ff44;
    box-shadow:
      0 0 4px rgba(68, 255, 68, 0.6),
      0 0 8px rgba(68, 255, 68, 0.4);
  }
</style>
