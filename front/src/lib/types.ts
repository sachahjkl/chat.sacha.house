export type Message = {
  id: string;
  username: string;
  text: string;
  created_at: number;
};

export type NotificationType = "error" | "success" | "info";

export type Notification = {
  id: string;
  message: string;
  type: NotificationType;
  createdAt: number;
};
