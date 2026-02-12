// Trimmed helper from legacy shared helpers.

export function bindModel(this: Record<string, unknown>, model: Record<string, unknown>): void {
  Object.assign(this, model);
}
