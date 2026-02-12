// Trimmed from:
// /Volumes/sp_backup/imagemation/migration/ng-15/WebApp.Desktop/src/app/shared_2023/models/meta-data.ts

export class MetaData {
  constructor(public rawData: unknown = {}) {}

  static toApi(instance: MetaData): MetaDataForApi {
    return new MetaDataForApi(instance);
  }
}

export class MetaDataForApi {
  constructor(public instance: MetaData) {}
}
