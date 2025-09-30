import toPath         from 'lodash/toPath';

import * as miscUtils from './miscUtils';
import * as nodeUtils from './nodeUtils';

export type ProcessResult = {
  manifestUpdates: Map<string, Map<string, Map<any, Set<nodeUtils.Caller>>>>;
  reportedErrors: Map<string, Array<string>>;
};

export interface Engine {
  process(): Promise<ProcessResult | null>;
}

export class Index<T extends {[key: string]: any}> {
  private items: Array<T> = [];

  private indexes: {
    [K in keyof T]?: Map<any, Array<T>>;
  } = {};

  constructor(private indexedFields: Array<keyof T>) {
    this.clear();
  }

  clear() {
    this.items = [];

    for (const field of this.indexedFields) {
      this.indexes[field] = new Map();
    }
  }

  insert(item: T) {
    this.items.push(item);

    for (const field of this.indexedFields) {
      const value = Object.hasOwn(item, field)
        ? item[field]
        : undefined;

      if (typeof value === `undefined`)
        continue;

      const list = miscUtils.getArrayWithDefault(this.indexes[field]!, value);
      list.push(item);
    }

    return item;
  }

  find(filter?: {[K in keyof T]?: any}) {
    if (typeof filter === `undefined`)
      return this.items;

    const filterEntries = Object.entries(filter);
    if (filterEntries.length === 0)
      return this.items;

    const sequentialFilters: Array<[keyof T, any]> = [];

    let matches: Set<T> | undefined;
    for (const [field_, value] of filterEntries) {
      const field = field_ as keyof T;

      const index = Object.hasOwn(this.indexes, field)
        ? this.indexes[field]
        : undefined;

      if (typeof index === `undefined`) {
        sequentialFilters.push([field, value]);
        continue;
      }

      const filterMatches = new Set(index.get(value) ?? []);
      if (filterMatches.size === 0)
        return [];

      if (typeof matches === `undefined`) {
        matches = filterMatches;
      } else {
        for (const item of matches) {
          if (!filterMatches.has(item)) {
            matches.delete(item);
          }
        }
      }

      if (matches.size === 0) {
        break;
      }
    }

    let result = [...matches ?? []];
    if (sequentialFilters.length > 0) {
      result = result.filter(item => {
        for (const [field, value] of sequentialFilters) {
          const valid = typeof value !== `undefined`
            ? Object.hasOwn(item, field) && item[field] === value
            : Object.hasOwn(item, field) === false;

          if (!valid) {
            return false;
          }
        }

        return true;
      });
    }

    return result;
  }
}

export function normalizePath(p: Array<string> | string) {
  return Array.isArray(p) ? p : toPath(p);
}
