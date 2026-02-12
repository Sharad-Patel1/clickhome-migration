// Trimmed from:
// /Volumes/sp_backup/imagemation/migration/ng-15/WebApp.Desktop/src/app/shared/models/doc-client-shares.ts

export class DocClientShare {
  constructor(model: Partial<DocClientShare> = {}) {
    this.key = model.key;
    this.code = model.code;
    this.label = model.label;
  }

  key?: string = undefined;
  code?: number = undefined;
  label?: string = undefined;
}
