// Trimmed consumer to exercise mixed legacy/modern imports for graph planning.
// Based on: /Volumes/sp_backup/imagemation/migration/ng-15/WebApp.Desktop/src/app

import { Collection } from '../shared/models/collection';
import { CollectionForApi } from '../shared/models/collection-for-api';
import {
  Collection as Collection2023,
  CollectionForApi as CollectionForApi2023,
} from '../shared_2023/models/collection';
import { File } from '../shared_2023/models/file';

export class CollectionConsumer {
  legacyCollection?: Collection;
  legacyCollectionForApi?: CollectionForApi;
  modernCollection?: Collection2023;
  modernCollectionForApi?: CollectionForApi2023;
  modernFile?: File;

  constructor() {
    this.legacyCollection = new Collection();
    this.modernCollection = new Collection2023();
  }
}
