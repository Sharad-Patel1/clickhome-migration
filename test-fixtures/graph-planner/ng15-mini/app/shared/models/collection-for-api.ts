// Trimmed legacy serializer wrapper used by doc-category-for-api.

import { Collection } from './collection';

export class CollectionForApi<T> extends Collection<T> {
  constructor(model: { list?: T[]; count?: number } = {}) {
    super(model);
  }
}
