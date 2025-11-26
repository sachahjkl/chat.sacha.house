import type { Notification, NotificationType } from "../types";

import { writable } from "svelte/store";

const SNACKBAR_DURATION_MS = 5000;

interface NotificationStore {
  notifications: Notification[];
}

function createNotificationStore() {
  const { subscribe, update } = writable<NotificationStore>({
    notifications: [],
  });

  let nextId = 0;

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

      update((state) => ({
        notifications: [...state.notifications, notification],
      }));

      setTimeout(() => {
        update((state) => ({
          notifications: state.notifications.filter((n) => n.id !== id),
        }));
      }, SNACKBAR_DURATION_MS);

      return id;
    },
    dismiss: (id: string) => {
      update((state) => ({
        notifications: state.notifications.filter((n) => n.id !== id),
      }));
    },
  };
}

export const notifications = createNotificationStore();
