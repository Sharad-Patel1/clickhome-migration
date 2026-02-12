// Trimmed from:
// /Volumes/sp_backup/imagemation/migration/ng-15/WebApp.Desktop/src/app/shared/models/doc-construction-client-shares.ts

export class DocConstructionShare {
  constructor(model: Partial<DocConstructionShare> = {}) {
    this.key = model.key;
    this.code = model.code;
    this.label = model.label;
  }

  key?: string = undefined;
  code?: number = undefined;
  label?: string = undefined;
}
