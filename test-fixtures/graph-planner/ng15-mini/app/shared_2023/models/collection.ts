// Trimmed modern model counterpart used for inventory/mapping coverage.

export class Collection<T = unknown> {
  constructor(public list: T[] = []) {}
}

export class CollectionForApi<T = unknown> {
  constructor(public list: T[] = []) {}
}
