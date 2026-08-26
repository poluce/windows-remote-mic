export type MappingEntry = {
  button: string;
  name: string;
  trigger: string;
  action: string;
  action_key: string;
};

export type RemoteButton = {
  key: string;
  name: string;
};

export type TriggerOption = {
  key: string;
  label: string;
  desc: string;
};

export type ActionItem = {
  key: string;
  label: string;
};

export type ActionCategory = {
  key: string;
  title: string;
  actions: ActionItem[];
};