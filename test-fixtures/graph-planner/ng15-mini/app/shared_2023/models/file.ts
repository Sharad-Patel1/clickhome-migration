// Trimmed from:
// /Volumes/sp_backup/imagemation/migration/ng-15/WebApp.Desktop/src/app/shared_2023/models/file.ts

import { MetaData } from './meta-data';

export class File {
  constructor(public rawData: unknown = {}) {}

  public metaData?: MetaData = undefined;

  static toApi(instance: File): FileForApi {
    return new FileForApi(instance);
  }
}

export class FileForApi {
  constructor(public instance: File) {}
}
