// Trimmed from:
// /Volumes/sp_backup/imagemation/migration/ng-15/WebApp.Desktop/src/app/components/document/models/doc-category.ts

import { FormControl, FormGroup } from '@angular/forms';
import { bindModel } from '../../../shared/helpers/bind-model';
import { DocCategoryModel } from '../../../shared/interfaces';
import { DocConstructionShare } from '../../../shared/models/doc-construction-client-shares';
import { DocClientShare } from '../../../shared/models/doc-client-shares';
import { MetaData, MetaDataForm } from '../../../shared/models/meta-data';
import { Collection } from '../../../shared/models/collection';
import { File } from '../../../shared_2023/models/file';

export class DocCategory {
  constructor(model?: DocCategory | DocCategoryModel) {
    if (typeof model === 'object') {
      bindModel.call(this, model, {
        files: (value) => (value ? new Collection(value, File) : undefined),
        metaData: (value) => (value ? new MetaData(value) : undefined),
        clientShare: (value) => new DocClientShare(value),
        constructionShare: (value) => new DocConstructionShare(value),
      });
    }
  }

  categoryName?: string = undefined;
  clientShare?: DocClientShare = undefined;
  constructionShare?: DocConstructionShare = undefined;
  files?: Collection<File> = undefined;
  metaData?: MetaData = undefined;
}

export class DocCategoryForm extends FormGroup {
  constructor(data?: DocCategory) {
    super(
      Object.entries(data || new DocCategory()).reduce((memo, [key, value]) => {
        switch (key) {
          case 'metaData':
            memo[key] = new MetaDataForm(value);
            break;
          default:
            memo[key] = new FormControl(value);
        }
        return memo;
      }, {})
    );
  }

  value: DocCategory;
}
