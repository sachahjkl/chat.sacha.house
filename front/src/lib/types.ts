export type Julid = string;

export type Message = {
  id: Julid;
  username: string;
  text: string;
  created_at: number;
};

export type NotificationType = "error" | "success" | "info";

export type Notification = {
  id: Julid;
  message: string;
  type: NotificationType;
  createdAt: number;
};

export type ClaimResponse = {
  success: boolean;
  session_id: Julid;
  username: string;
  expires_in: number;
};

export type UsersResponse = {
  users: string[];
};

export type UserEventAction = "ADD" | "REMOVE";

export type UserEvent = {
  action: UserEventAction;
  username: string;
};

export type CurrentUsernameResponse = {
  username: string | null;
  expires_in: number | null;
};

export type MessagesResponse = {
  messages: Message[];
};

export type PostMessageResponse = {
  success: boolean;
  message_id: Julid;
};

export type UsernameReleaseResponse = {
  success: boolean;
};

export type StatsResponse = {
  total_messages: number;
};

export type Username = string;

export type IntervalHandle = ReturnType<typeof setInterval>;

export type Settings = {
  autoReclaimEnabled: boolean;
};

export type ErrorResponse = {
  error: string;
};