// Trimmed from:
// /Volumes/sp_backup/imagemation/migration/ng-15/WebApp.Desktop/src/app/shared/models/collection.ts

export class Collection<T> {
  constructor(model: { list?: T[]; count?: number } = {}) {
    this.list = model.list;
    this.count = model.count ?? model.list?.length;
  }

  public list?: T[] = undefined;
  public count?: number = undefined;
}
