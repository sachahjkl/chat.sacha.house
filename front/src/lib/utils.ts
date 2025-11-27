export function else_if_NaN(value: unknown, defaultValue: number): number {
  if (typeof value === "number" && !Number.isNaN(value)) {
    return value;
  }
  return defaultValue;
}
