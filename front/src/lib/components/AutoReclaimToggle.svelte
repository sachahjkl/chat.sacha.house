<script lang="ts">
  interface Props {
    enabled: boolean;
    label?: string;
    title?: string;
    onToggle?: (enabled: boolean) => void;
  }

  let { enabled = $bindable(), label, title = "Toggle auto reclaim", onToggle = () => {} }: Props = $props();

  function handleToggle(event: Event) {
    const target = event.target as HTMLInputElement;
    enabled = target.checked;
    onToggle(target.checked);
  }
</script>

<div class="toggle-container">
  <div>
    <label class="toggle" {title}>
      <input type="checkbox" checked={enabled} onchange={handleToggle} />
      <span class="slider"> </span>
      {#if label}{/if}
    </label>
  </div>
  <div class="label">{label}</div>
</div>

<style>
  .toggle-container {
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .toggle {
    font-size: 0.85rem;
    cursor: pointer;
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: center;
    font-size: 0.5rem;
  }

  .toggle input {
    position: absolute;
    opacity: 0;
    width: 0;
    height: 0;
  }

  .slider {
    position: relative;
    display: block;
    width: 36px;
    height: 20px;
    background: #2a2a2a;
    border: 1px solid #3a3a3a;
    transition: background 0.15s;
  }

  .slider::before {
    content: "";
    position: absolute;
    width: calc(100% / 2 - 1px);
    height: calc(100% - 2px);
    left: 1px;
    top: 1px;
    background: #666;
    transition: transform 0.15s;
  }

  .toggle input:checked + .slider {
    background: #224a22;
    border-color: #44ff44;
  }

  .toggle input:checked + .slider::before {
    transform: translateX(100%);
    background: #44ff44;
  }
</style>
