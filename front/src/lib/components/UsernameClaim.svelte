<script lang="ts">
  import AutoReclaimToggle from "./AutoReclaimToggle.svelte";

  interface Props {
    username: string;
    claimed: boolean;
    claiming: boolean;
    autoReclaimEnabled: boolean;
    remainingSeconds: number | null;
    onClaim: (username: string) => void;
    onRelease: () => void;
    onToggleAutoReclaim: (enabled: boolean) => void;
  }

  let {
    username = $bindable(),
    claimed,
    claiming,
    autoReclaimEnabled = $bindable(),
    remainingSeconds,
    onClaim,
    onRelease,
    onToggleAutoReclaim,
  }: Props = $props();

  function handleClaim(event: SubmitEvent) {
    event.preventDefault();
    if (!username.trim()) return;
    onClaim(username.trim());
  }

  function handleManualClick() {
    if (autoReclaimEnabled) {
      onRelease();
    } else {
      onClaim(username.trim());
    }
  }
</script>

<form class="claim" onsubmit={handleClaim}>
  <input
    name="username"
    title="Username"
    aria-label="Username"
    placeholder="Pick a username"
    bind:value={username}
    disabled={claiming || claimed}
    autocomplete="off"
  />
  {#if !claimed}
    <div class="input-group justify-end">
      <button class="text-sm" type="submit" disabled={claiming || !username.trim()}>
        {claiming ? "Claiming…" : "Claim username"}
      </button>
    </div>
  {:else}
    <div class="input-group text-sm justify-between">
      <p class="claimed grow">🔒 username "{username}" locked for this session.</p>
      <div class="flex-align-center justify-end grow">
        <AutoReclaimToggle
          bind:enabled={autoReclaimEnabled}
          title="Toggle auto reclaim"
          label="Auto reclaim"
          onToggle={onToggleAutoReclaim}
        />
        <button
          type="button"
          class:release={autoReclaimEnabled}
          class:claim={!autoReclaimEnabled}
          onclick={handleManualClick}
        >
          {autoReclaimEnabled ? "Manual release" : "Manual claim"}
          {#if remainingSeconds !== null}
            (auto {autoReclaimEnabled ? "reclaim" : "release"} in {remainingSeconds}s){/if}
        </button>
      </div>
    </div>
  {/if}
</form>

<style>
  form {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  input {
    border-radius: 0;
    border: 1px solid #2a2a2a;
    padding: 0.625rem;
    font: inherit;
    background: #0a0a0a;
    color: #e0e0e0;
    transition: border-color 0.15s;
  }

  input:hover {
    border-color: #3a3a3a;
  }

  input:focus {
    outline: none;
    border-color: #4a4a4a;
    color: #fff;
  }

  input:disabled {
    opacity: 0.5;
    cursor: not-allowed;
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

  button.release {
    background: #2a1111;
    border-color: #4a2222;
    color: #ff8888;
  }

  button.release:hover:not(:disabled) {
    background: #3a1a1a;
    border-color: #5a3333;
  }

  button.claim {
    background: #112a11;
    border-color: #224a22;
    color: #88ff88;
  }

  button.claim:hover:not(:disabled) {
    background: #1a3a1a;
    border-color: #335a33;
  }

  .input-group {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }

  .justify-between {
    justify-content: space-between;
  }

  .justify-end {
    justify-content: flex-end;
  }

  .flex-align-center {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
  }

  .grow {
    flex-grow: 1;
  }

  .claimed {
    margin: 0;
    font-size: 0.9rem;
    color: #44ff44;
    padding: 0.5rem;
    background: #112a11;
    border: 1px solid #224a22;
  }

  .text-sm {
    font-size: 0.85rem;
  }
</style>
