<script lang="ts">
  import { alerts, dismissAlert } from "../lib/store";
</script>

{#if $alerts.length > 0}
  <div class="alert-bar">
    {#each $alerts as alert (alert.id)}
      <div class="alert-item {alert.level}">
        <span class="alert-icon">
          {alert.level === "error" ? "❌" : alert.level === "warning" ? "⚠️" : alert.level === "meeting" ? "📞" : "ℹ️"}
        </span>
        <span class="alert-message">{alert.message}</span>
        <div class="alert-actions-row">
          {#if alert.actions}
            {#each alert.actions as action}
              <button class="alert-action" onclick={() => { action.handler(); }}>{action.label}</button>
            {/each}
          {/if}
          {#if alert.dismissable}
            <button class="alert-dismiss" onclick={() => dismissAlert(alert.id)}>✕</button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .alert-bar {
    display: flex; flex-direction: column; gap: 4px;
    animation: slideDown 0.2s ease;
  }
  @keyframes slideDown { from { opacity: 0; transform: translateY(-8px); } to { opacity: 1; transform: translateY(0); } }
  .alert-item {
    display: flex; align-items: center; gap: 10px;
    padding: 10px 14px; border-radius: 8px;
    font-size: 13px; line-height: 1.4;
  }
  .alert-item.error {
    background: rgba(248, 113, 113, 0.12); border: 1px solid rgba(248, 113, 113, 0.3); color: #fca5a5;
  }
  .alert-item.warning {
    background: rgba(251, 191, 36, 0.1); border: 1px solid rgba(251, 191, 36, 0.25); color: #fde68a;
  }
  .alert-item.info {
    background: rgba(96, 165, 250, 0.1); border: 1px solid rgba(96, 165, 250, 0.25); color: #93c5fd;
  }
  .alert-item.meeting {
    background: rgba(124, 108, 255, 0.1); border: 1px solid rgba(124, 108, 255, 0.3); color: #c4b5fd;
  }
  .alert-icon { font-size: 16px; flex-shrink: 0; }
  .alert-message { flex: 1; }
  .alert-actions-row { display: flex; gap: 6px; align-items: center; flex-shrink: 0; }
  .alert-action {
    background: rgba(255,255,255,0.1); border: 1px solid rgba(255,255,255,0.15);
    color: inherit; padding: 4px 12px; border-radius: 5px;
    font-size: 12px; font-weight: 600; cursor: pointer;
    font-family: inherit; white-space: nowrap;
    transition: background 0.15s;
  }
  .alert-action:hover { background: rgba(255,255,255,0.2); }
  .alert-dismiss {
    background: none; border: none; color: inherit; opacity: 0.5;
    cursor: pointer; font-size: 14px; padding: 2px 4px; font-family: inherit;
  }
  .alert-dismiss:hover { opacity: 1; }
</style>
