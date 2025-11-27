import type { IntervalHandle, Notification, NotificationType } from "../types";

import { writable } from "svelte/store";

const SNACKBAR_DURATION_MS = 5000;
const DEFAULT_MAX_ACTIVE_NOTIFICATIONS = 3;

interface NotificationStore {
  notifications: Notification[];
}

export function createNotificationStore(maxActive = DEFAULT_MAX_ACTIVE_NOTIFICATIONS) {
  const { subscribe, update } = writable<NotificationStore>({
    notifications: [],
  });

  let nextId = 0;
  const timers = new Map<string, IntervalHandle>();

  const scheduleAutoDismiss = (id: string) => {
    const timer = setTimeout(() => {
      timers.delete(id);
      update((state) => ({
        notifications: state.notifications.filter((n) => n.id !== id),
      }));
    }, SNACKBAR_DURATION_MS);

    timers.set(id, timer);
  };

  const clearTimer = (id: string) => {
    const timer = timers.get(id);
    if (timer) {
      clearTimeout(timer);
      timers.delete(id);
    }
  };

  const enqueue = (notification: Notification) => {
    update((state) => {
      const next = [...state.notifications, notification];
      while (next.length > maxActive) {
        const removed = next.shift();
        if (removed) {
          clearTimer(removed.id);
        }
      }
      return { notifications: next };
    });
  };

  return {
    subscribe,
    showNotification: (message: string, type: NotificationType = "error") => {
      const id = `notification-${nextId++}`;
      const notification: Notification = {
        id,
        message,
        type,
        createdAt: Date.now(),
      };

      enqueue(notification);
      scheduleAutoDismiss(id);

      return id;
    },
    dismiss: (id: string) => {
      clearTimer(id);
      update((state) => ({
        notifications: state.notifications.filter((n) => n.id !== id),
      }));
    },
  };
}

export const notifications = createNotificationStore(3);
