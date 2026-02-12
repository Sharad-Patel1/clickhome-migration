// Trimmed from:
// /Volumes/sp_backup/imagemation/migration/ng-15/WebApp.Desktop/src/app/shared/models/meta-data.ts

import { bindModel } from '../helpers/bind-model';

export class MetaData {
  constructor(model?: MetaData) {
    if (typeof model === 'object') {
      bindModel.call(this, model);
    }
  }

  public active?: boolean = undefined;
  public createdOn?: string = undefined;
}

export class MetaDataForm {
  constructor(public readonly value: MetaData = new MetaData()) {}
}
