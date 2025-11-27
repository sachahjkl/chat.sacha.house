import { SvelteSet } from "svelte/reactivity";

export class TopKStore<Key, Value> {
  #maxSize: number;
  #ids = new SvelteSet<Key>();
  #compare: (a: Key, b: Key) => number;
  #keySelector: (item: Value) => Key;

  items = $state<Value[]>([]);

  constructor(
    maxSize: number,
    keySelector: (item: Value) => Key,
    compare: (a: Key, b: Key) => number = TopKStore.descending()
  ) {
    this.#maxSize = maxSize;
    this.#compare = compare;
    this.#keySelector = keySelector;
  }

  insert(item: Value): boolean {
    const itemId = this.#keySelector(item);
    if (this.#ids.has(itemId)) {
      return false;
    }

    let low = 0;
    let high = this.items.length;

    while (low < high) {
      const mid = (low + high) >> 1;
      const midId = this.#keySelector(this.items[mid]);
      if (this.#compare(itemId, midId) < 0) {
        high = mid;
      } else {
        low = mid + 1;
      }
    }

    this.items.splice(low, 0, item);
    this.#ids.add(itemId);

    if (this.items.length > this.#maxSize) {
      const removed = this.items.pop();
      if (removed) {
        this.#ids.delete(this.#keySelector(removed));
      }
    }

    return true;
  }

  load(items: Value[]): void {
    this.clear();
    for (const item of items) {
      this.insert(item);
    }
  }

  clear(): void {
    this.items.length = 0;
    this.#ids.clear();
  }

  get size(): number {
    return this.items.length;
  }

  has(id: Key): boolean {
    return this.#ids.has(id);
  }

  static descending<Key, Value>(
    selector: (x: Value) => Key = (x) => x as unknown as Key
  ): (a: Value, b: Value) => number {
    return (a, b) => {
      const av = selector(a);
      const bv = selector(b);
      if (av === bv) return 0;
      return av > bv ? -1 : 1;
    };
  }

  static ascending<Key, Value>(
    selector: (x: Value) => Key = (x) => x as unknown as Key
  ): (a: Value, b: Value) => number {
    return (a, b) => {
      const av = selector(a);
      const bv = selector(b);
      if (av === bv) return 0;
      return av < bv ? -1 : 1;
    };
  }
}
