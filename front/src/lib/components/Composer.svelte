<script lang="ts">
  import { TextareaAutosize } from "runed";
  import { tick } from "svelte";
  import { fly } from "svelte/transition";

  interface Props {
    claimed: boolean;
    messageText: string;
    onSubmit: (text: string) => void;
    showScrollToTop: boolean;
    onScrollToTop: () => void;
    onfocus?: () => void;
    onblur?: () => void;
  }

  let form : HTMLFormElement;

  const MAX_MESSAGE_LENGTH = 240;

  export async function focusTextarea() {
    if (textarea) {
      await tick();
      textarea.focus();
    }
  }

  let {
    claimed,
    messageText = $bindable(),
    onSubmit,
    showScrollToTop,
    onScrollToTop,
    onfocus = () => {},
    onblur = () => {},
  }: Props = $props();

  let textarea: HTMLTextAreaElement;
  new TextareaAutosize({
    element: () => textarea,
    input: () => messageText,
    maxHeight: 120,
  });

  function handleComposerKey(event: KeyboardEvent) {
    if (event.key === "Enter" && event.ctrlKey) {
      event.preventDefault();
      form.requestSubmit();
    }
  }

  function handleSubmit(event?: Event) {
    event?.preventDefault();
    if (!messageText.trim() || !claimed) return;
    onSubmit(messageText.trim());
    messageText = "";
  }
</script>

<form bind:this={form} class="composer" onsubmit={handleSubmit}>
  <textarea
    bind:this={textarea}
    placeholder={claimed ? "Say something nice" : "Claim a username first"}
    bind:value={messageText}
    maxlength={MAX_MESSAGE_LENGTH}
    disabled={!claimed}
    name="message-text"
    onkeydown={handleComposerKey}
    {onfocus}
    {onblur}
  ></textarea>
  <div class="composer__meta">
    <span>{messageText.trim().length}/240</span>
    <div class="composer__actions">
      {#if showScrollToTop}
        <button
          aria-label="Scroll to top"
          title="Scroll to top"
          type="button"
          class="scroll-to-top"
          onclick={onScrollToTop}
          transition:fly={{ y: 20, duration: 200 }}
        >
          ↑
        </button>
      {/if}
      <button type="submit" disabled={!claimed || !messageText.trim()}>Send</button>
    </div>
  </div>
</form>

<style>
  .composer {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    background: #111;
    z-index: 100;
  }

  textarea {
    border-radius: 0;
    border: 1px solid #2a2a2a;
    padding: 0.625rem;
    font: inherit;
    background: #0a0a0a;
    color: #e0e0e0;
    transition: border-color 0.15s;
    overflow-y: hidden;
    resize: none;
  }

  textarea:hover {
    border-color: #3a3a3a;
  }

  textarea:focus {
    outline: none;
    border-color: #4a4a4a;
    color: #fff;
  }

  textarea:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .composer__meta {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.85rem;
    color: #aaa;
  }

  .composer__meta span {
    color: #888;
  }

  .composer__actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .scroll-to-top {
    padding: 0.5rem;
    min-width: auto;
    font-size: 1.2rem;
    line-height: 1;
  }

  button {
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

  button:hover:not(:disabled) {
    background: #222;
    border-color: #3a3a3a;
  }

  button:active:not(:disabled) {
    background: #0f0f0f;
  }

  button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>
