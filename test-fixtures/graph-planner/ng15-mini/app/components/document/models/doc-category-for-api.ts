// Trimmed from:
// /Volumes/sp_backup/imagemation/migration/ng-15/WebApp.Desktop/src/app/components/document/models/doc-category-for-api.ts

import { bindModel } from '../../../shared/helpers/bind-model';
import { CollectionForApi } from '../../../shared/models/collection-for-api';
import { MetaData } from '../../../shared/models/meta-data';
import { DocCategory } from './doc-category';
import { FileForApi } from '../../../shared_2023/models/file';

export class DocCategoryForApi {
  constructor(model?: DocCategory) {
    if (typeof model === 'object') {
      bindModel.call(this, model, {
        files: (value) => (value ? new CollectionForApi(value, FileForApi) : undefined),
        metaData: (value) => (value ? new MetaData(value) : undefined),
        clientShare: (value) => value?.code ?? value,
        constructionShare: (value) => value?.code ?? value,
      });
    }
  }

  categoryName?: string = undefined;
  clientShare?: number = undefined;
  constructionShare?: number = undefined;
  files?: CollectionForApi<FileForApi> = undefined;
}
