"use strict";
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __commonJS = (cb, mod) => function __require() {
  return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/isArray.js
var require_isArray = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/isArray.js"(exports2, module2) {
    var isArray = Array.isArray;
    module2.exports = isArray;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_freeGlobal.js
var require_freeGlobal = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_freeGlobal.js"(exports2, module2) {
    var freeGlobal = typeof global == "object" && global && global.Object === Object && global;
    module2.exports = freeGlobal;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_root.js
var require_root = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_root.js"(exports2, module2) {
    var freeGlobal = require_freeGlobal();
    var freeSelf = typeof self == "object" && self && self.Object === Object && self;
    var root = freeGlobal || freeSelf || Function("return this")();
    module2.exports = root;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_Symbol.js
var require_Symbol = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_Symbol.js"(exports2, module2) {
    var root = require_root();
    var Symbol2 = root.Symbol;
    module2.exports = Symbol2;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_getRawTag.js
var require_getRawTag = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_getRawTag.js"(exports2, module2) {
    var Symbol2 = require_Symbol();
    var objectProto = Object.prototype;
    var hasOwnProperty = objectProto.hasOwnProperty;
    var nativeObjectToString = objectProto.toString;
    var symToStringTag = Symbol2 ? Symbol2.toStringTag : void 0;
    function getRawTag(value) {
      var isOwn = hasOwnProperty.call(value, symToStringTag), tag = value[symToStringTag];
      try {
        value[symToStringTag] = void 0;
        var unmasked = true;
      } catch (e) {
      }
      var result = nativeObjectToString.call(value);
      if (unmasked) {
        if (isOwn) {
          value[symToStringTag] = tag;
        } else {
          delete value[symToStringTag];
        }
      }
      return result;
    }
    module2.exports = getRawTag;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_objectToString.js
var require_objectToString = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_objectToString.js"(exports2, module2) {
    var objectProto = Object.prototype;
    var nativeObjectToString = objectProto.toString;
    function objectToString(value) {
      return nativeObjectToString.call(value);
    }
    module2.exports = objectToString;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_baseGetTag.js
var require_baseGetTag = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_baseGetTag.js"(exports2, module2) {
    var Symbol2 = require_Symbol();
    var getRawTag = require_getRawTag();
    var objectToString = require_objectToString();
    var nullTag = "[object Null]";
    var undefinedTag = "[object Undefined]";
    var symToStringTag = Symbol2 ? Symbol2.toStringTag : void 0;
    function baseGetTag(value) {
      if (value == null) {
        return value === void 0 ? undefinedTag : nullTag;
      }
      return symToStringTag && symToStringTag in Object(value) ? getRawTag(value) : objectToString(value);
    }
    module2.exports = baseGetTag;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/isObjectLike.js
var require_isObjectLike = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/isObjectLike.js"(exports2, module2) {
    function isObjectLike(value) {
      return value != null && typeof value == "object";
    }
    module2.exports = isObjectLike;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/isSymbol.js
var require_isSymbol = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/isSymbol.js"(exports2, module2) {
    var baseGetTag = require_baseGetTag();
    var isObjectLike = require_isObjectLike();
    var symbolTag = "[object Symbol]";
    function isSymbol(value) {
      return typeof value == "symbol" || isObjectLike(value) && baseGetTag(value) == symbolTag;
    }
    module2.exports = isSymbol;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_isKey.js
var require_isKey = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_isKey.js"(exports2, module2) {
    var isArray = require_isArray();
    var isSymbol = require_isSymbol();
    var reIsDeepProp = /\.|\[(?:[^[\]]*|(["'])(?:(?!\1)[^\\]|\\.)*?\1)\]/;
    var reIsPlainProp = /^\w*$/;
    function isKey(value, object) {
      if (isArray(value)) {
        return false;
      }
      var type = typeof value;
      if (type == "number" || type == "symbol" || type == "boolean" || value == null || isSymbol(value)) {
        return true;
      }
      return reIsPlainProp.test(value) || !reIsDeepProp.test(value) || object != null && value in Object(object);
    }
    module2.exports = isKey;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/isObject.js
var require_isObject = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/isObject.js"(exports2, module2) {
    function isObject(value) {
      var type = typeof value;
      return value != null && (type == "object" || type == "function");
    }
    module2.exports = isObject;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/isFunction.js
var require_isFunction = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/isFunction.js"(exports2, module2) {
    var baseGetTag = require_baseGetTag();
    var isObject = require_isObject();
    var asyncTag = "[object AsyncFunction]";
    var funcTag = "[object Function]";
    var genTag = "[object GeneratorFunction]";
    var proxyTag = "[object Proxy]";
    function isFunction(value) {
      if (!isObject(value)) {
        return false;
      }
      var tag = baseGetTag(value);
      return tag == funcTag || tag == genTag || tag == asyncTag || tag == proxyTag;
    }
    module2.exports = isFunction;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_coreJsData.js
var require_coreJsData = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_coreJsData.js"(exports2, module2) {
    var root = require_root();
    var coreJsData = root["__core-js_shared__"];
    module2.exports = coreJsData;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_isMasked.js
var require_isMasked = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_isMasked.js"(exports2, module2) {
    var coreJsData = require_coreJsData();
    var maskSrcKey = function() {
      var uid = /[^.]+$/.exec(coreJsData && coreJsData.keys && coreJsData.keys.IE_PROTO || "");
      return uid ? "Symbol(src)_1." + uid : "";
    }();
    function isMasked(func) {
      return !!maskSrcKey && maskSrcKey in func;
    }
    module2.exports = isMasked;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_toSource.js
var require_toSource = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_toSource.js"(exports2, module2) {
    var funcProto = Function.prototype;
    var funcToString = funcProto.toString;
    function toSource(func) {
      if (func != null) {
        try {
          return funcToString.call(func);
        } catch (e) {
        }
        try {
          return func + "";
        } catch (e) {
        }
      }
      return "";
    }
    module2.exports = toSource;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_baseIsNative.js
var require_baseIsNative = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_baseIsNative.js"(exports2, module2) {
    var isFunction = require_isFunction();
    var isMasked = require_isMasked();
    var isObject = require_isObject();
    var toSource = require_toSource();
    var reRegExpChar = /[\\^$.*+?()[\]{}|]/g;
    var reIsHostCtor = /^\[object .+?Constructor\]$/;
    var funcProto = Function.prototype;
    var objectProto = Object.prototype;
    var funcToString = funcProto.toString;
    var hasOwnProperty = objectProto.hasOwnProperty;
    var reIsNative = RegExp(
      "^" + funcToString.call(hasOwnProperty).replace(reRegExpChar, "\\$&").replace(/hasOwnProperty|(function).*?(?=\\\()| for .+?(?=\\\])/g, "$1.*?") + "$"
    );
    function baseIsNative(value) {
      if (!isObject(value) || isMasked(value)) {
        return false;
      }
      var pattern = isFunction(value) ? reIsNative : reIsHostCtor;
      return pattern.test(toSource(value));
    }
    module2.exports = baseIsNative;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_getValue.js
var require_getValue = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_getValue.js"(exports2, module2) {
    function getValue(object, key) {
      return object == null ? void 0 : object[key];
    }
    module2.exports = getValue;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_getNative.js
var require_getNative = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_getNative.js"(exports2, module2) {
    var baseIsNative = require_baseIsNative();
    var getValue = require_getValue();
    function getNative(object, key) {
      var value = getValue(object, key);
      return baseIsNative(value) ? value : void 0;
    }
    module2.exports = getNative;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_nativeCreate.js
var require_nativeCreate = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_nativeCreate.js"(exports2, module2) {
    var getNative = require_getNative();
    var nativeCreate = getNative(Object, "create");
    module2.exports = nativeCreate;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_hashClear.js
var require_hashClear = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_hashClear.js"(exports2, module2) {
    var nativeCreate = require_nativeCreate();
    function hashClear() {
      this.__data__ = nativeCreate ? nativeCreate(null) : {};
      this.size = 0;
    }
    module2.exports = hashClear;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_hashDelete.js
var require_hashDelete = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_hashDelete.js"(exports2, module2) {
    function hashDelete(key) {
      var result = this.has(key) && delete this.__data__[key];
      this.size -= result ? 1 : 0;
      return result;
    }
    module2.exports = hashDelete;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_hashGet.js
var require_hashGet = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_hashGet.js"(exports2, module2) {
    var nativeCreate = require_nativeCreate();
    var HASH_UNDEFINED = "__lodash_hash_undefined__";
    var objectProto = Object.prototype;
    var hasOwnProperty = objectProto.hasOwnProperty;
    function hashGet(key) {
      var data = this.__data__;
      if (nativeCreate) {
        var result = data[key];
        return result === HASH_UNDEFINED ? void 0 : result;
      }
      return hasOwnProperty.call(data, key) ? data[key] : void 0;
    }
    module2.exports = hashGet;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_hashHas.js
var require_hashHas = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_hashHas.js"(exports2, module2) {
    var nativeCreate = require_nativeCreate();
    var objectProto = Object.prototype;
    var hasOwnProperty = objectProto.hasOwnProperty;
    function hashHas(key) {
      var data = this.__data__;
      return nativeCreate ? data[key] !== void 0 : hasOwnProperty.call(data, key);
    }
    module2.exports = hashHas;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_hashSet.js
var require_hashSet = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_hashSet.js"(exports2, module2) {
    var nativeCreate = require_nativeCreate();
    var HASH_UNDEFINED = "__lodash_hash_undefined__";
    function hashSet(key, value) {
      var data = this.__data__;
      this.size += this.has(key) ? 0 : 1;
      data[key] = nativeCreate && value === void 0 ? HASH_UNDEFINED : value;
      return this;
    }
    module2.exports = hashSet;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_Hash.js
var require_Hash = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_Hash.js"(exports2, module2) {
    var hashClear = require_hashClear();
    var hashDelete = require_hashDelete();
    var hashGet = require_hashGet();
    var hashHas = require_hashHas();
    var hashSet = require_hashSet();
    function Hash(entries) {
      var index = -1, length = entries == null ? 0 : entries.length;
      this.clear();
      while (++index < length) {
        var entry = entries[index];
        this.set(entry[0], entry[1]);
      }
    }
    Hash.prototype.clear = hashClear;
    Hash.prototype["delete"] = hashDelete;
    Hash.prototype.get = hashGet;
    Hash.prototype.has = hashHas;
    Hash.prototype.set = hashSet;
    module2.exports = Hash;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_listCacheClear.js
var require_listCacheClear = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_listCacheClear.js"(exports2, module2) {
    function listCacheClear() {
      this.__data__ = [];
      this.size = 0;
    }
    module2.exports = listCacheClear;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/eq.js
var require_eq = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/eq.js"(exports2, module2) {
    function eq(value, other) {
      return value === other || value !== value && other !== other;
    }
    module2.exports = eq;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_assocIndexOf.js
var require_assocIndexOf = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_assocIndexOf.js"(exports2, module2) {
    var eq = require_eq();
    function assocIndexOf(array, key) {
      var length = array.length;
      while (length--) {
        if (eq(array[length][0], key)) {
          return length;
        }
      }
      return -1;
    }
    module2.exports = assocIndexOf;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_listCacheDelete.js
var require_listCacheDelete = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_listCacheDelete.js"(exports2, module2) {
    var assocIndexOf = require_assocIndexOf();
    var arrayProto = Array.prototype;
    var splice = arrayProto.splice;
    function listCacheDelete(key) {
      var data = this.__data__, index = assocIndexOf(data, key);
      if (index < 0) {
        return false;
      }
      var lastIndex = data.length - 1;
      if (index == lastIndex) {
        data.pop();
      } else {
        splice.call(data, index, 1);
      }
      --this.size;
      return true;
    }
    module2.exports = listCacheDelete;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_listCacheGet.js
var require_listCacheGet = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_listCacheGet.js"(exports2, module2) {
    var assocIndexOf = require_assocIndexOf();
    function listCacheGet(key) {
      var data = this.__data__, index = assocIndexOf(data, key);
      return index < 0 ? void 0 : data[index][1];
    }
    module2.exports = listCacheGet;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_listCacheHas.js
var require_listCacheHas = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_listCacheHas.js"(exports2, module2) {
    var assocIndexOf = require_assocIndexOf();
    function listCacheHas(key) {
      return assocIndexOf(this.__data__, key) > -1;
    }
    module2.exports = listCacheHas;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_listCacheSet.js
var require_listCacheSet = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_listCacheSet.js"(exports2, module2) {
    var assocIndexOf = require_assocIndexOf();
    function listCacheSet(key, value) {
      var data = this.__data__, index = assocIndexOf(data, key);
      if (index < 0) {
        ++this.size;
        data.push([key, value]);
      } else {
        data[index][1] = value;
      }
      return this;
    }
    module2.exports = listCacheSet;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_ListCache.js
var require_ListCache = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_ListCache.js"(exports2, module2) {
    var listCacheClear = require_listCacheClear();
    var listCacheDelete = require_listCacheDelete();
    var listCacheGet = require_listCacheGet();
    var listCacheHas = require_listCacheHas();
    var listCacheSet = require_listCacheSet();
    function ListCache(entries) {
      var index = -1, length = entries == null ? 0 : entries.length;
      this.clear();
      while (++index < length) {
        var entry = entries[index];
        this.set(entry[0], entry[1]);
      }
    }
    ListCache.prototype.clear = listCacheClear;
    ListCache.prototype["delete"] = listCacheDelete;
    ListCache.prototype.get = listCacheGet;
    ListCache.prototype.has = listCacheHas;
    ListCache.prototype.set = listCacheSet;
    module2.exports = ListCache;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_Map.js
var require_Map = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_Map.js"(exports2, module2) {
    var getNative = require_getNative();
    var root = require_root();
    var Map2 = getNative(root, "Map");
    module2.exports = Map2;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_mapCacheClear.js
var require_mapCacheClear = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_mapCacheClear.js"(exports2, module2) {
    var Hash = require_Hash();
    var ListCache = require_ListCache();
    var Map2 = require_Map();
    function mapCacheClear() {
      this.size = 0;
      this.__data__ = {
        "hash": new Hash(),
        "map": new (Map2 || ListCache)(),
        "string": new Hash()
      };
    }
    module2.exports = mapCacheClear;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_isKeyable.js
var require_isKeyable = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_isKeyable.js"(exports2, module2) {
    function isKeyable(value) {
      var type = typeof value;
      return type == "string" || type == "number" || type == "symbol" || type == "boolean" ? value !== "__proto__" : value === null;
    }
    module2.exports = isKeyable;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_getMapData.js
var require_getMapData = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_getMapData.js"(exports2, module2) {
    var isKeyable = require_isKeyable();
    function getMapData(map, key) {
      var data = map.__data__;
      return isKeyable(key) ? data[typeof key == "string" ? "string" : "hash"] : data.map;
    }
    module2.exports = getMapData;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_mapCacheDelete.js
var require_mapCacheDelete = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_mapCacheDelete.js"(exports2, module2) {
    var getMapData = require_getMapData();
    function mapCacheDelete(key) {
      var result = getMapData(this, key)["delete"](key);
      this.size -= result ? 1 : 0;
      return result;
    }
    module2.exports = mapCacheDelete;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_mapCacheGet.js
var require_mapCacheGet = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_mapCacheGet.js"(exports2, module2) {
    var getMapData = require_getMapData();
    function mapCacheGet(key) {
      return getMapData(this, key).get(key);
    }
    module2.exports = mapCacheGet;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_mapCacheHas.js
var require_mapCacheHas = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_mapCacheHas.js"(exports2, module2) {
    var getMapData = require_getMapData();
    function mapCacheHas(key) {
      return getMapData(this, key).has(key);
    }
    module2.exports = mapCacheHas;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_mapCacheSet.js
var require_mapCacheSet = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_mapCacheSet.js"(exports2, module2) {
    var getMapData = require_getMapData();
    function mapCacheSet(key, value) {
      var data = getMapData(this, key), size = data.size;
      data.set(key, value);
      this.size += data.size == size ? 0 : 1;
      return this;
    }
    module2.exports = mapCacheSet;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_MapCache.js
var require_MapCache = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_MapCache.js"(exports2, module2) {
    var mapCacheClear = require_mapCacheClear();
    var mapCacheDelete = require_mapCacheDelete();
    var mapCacheGet = require_mapCacheGet();
    var mapCacheHas = require_mapCacheHas();
    var mapCacheSet = require_mapCacheSet();
    function MapCache(entries) {
      var index = -1, length = entries == null ? 0 : entries.length;
      this.clear();
      while (++index < length) {
        var entry = entries[index];
        this.set(entry[0], entry[1]);
      }
    }
    MapCache.prototype.clear = mapCacheClear;
    MapCache.prototype["delete"] = mapCacheDelete;
    MapCache.prototype.get = mapCacheGet;
    MapCache.prototype.has = mapCacheHas;
    MapCache.prototype.set = mapCacheSet;
    module2.exports = MapCache;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/memoize.js
var require_memoize = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/memoize.js"(exports2, module2) {
    var MapCache = require_MapCache();
    var FUNC_ERROR_TEXT = "Expected a function";
    function memoize(func, resolver) {
      if (typeof func != "function" || resolver != null && typeof resolver != "function") {
        throw new TypeError(FUNC_ERROR_TEXT);
      }
      var memoized = function() {
        var args = arguments, key = resolver ? resolver.apply(this, args) : args[0], cache = memoized.cache;
        if (cache.has(key)) {
          return cache.get(key);
        }
        var result = func.apply(this, args);
        memoized.cache = cache.set(key, result) || cache;
        return result;
      };
      memoized.cache = new (memoize.Cache || MapCache)();
      return memoized;
    }
    memoize.Cache = MapCache;
    module2.exports = memoize;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_memoizeCapped.js
var require_memoizeCapped = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_memoizeCapped.js"(exports2, module2) {
    var memoize = require_memoize();
    var MAX_MEMOIZE_SIZE = 500;
    function memoizeCapped(func) {
      var result = memoize(func, function(key) {
        if (cache.size === MAX_MEMOIZE_SIZE) {
          cache.clear();
        }
        return key;
      });
      var cache = result.cache;
      return result;
    }
    module2.exports = memoizeCapped;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_stringToPath.js
var require_stringToPath = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_stringToPath.js"(exports2, module2) {
    var memoizeCapped = require_memoizeCapped();
    var rePropName = /[^.[\]]+|\[(?:(-?\d+(?:\.\d+)?)|(["'])((?:(?!\2)[^\\]|\\.)*?)\2)\]|(?=(?:\.|\[\])(?:\.|\[\]|$))/g;
    var reEscapeChar = /\\(\\)?/g;
    var stringToPath = memoizeCapped(function(string) {
      var result = [];
      if (string.charCodeAt(0) === 46) {
        result.push("");
      }
      string.replace(rePropName, function(match, number, quote, subString) {
        result.push(quote ? subString.replace(reEscapeChar, "$1") : number || match);
      });
      return result;
    });
    module2.exports = stringToPath;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_arrayMap.js
var require_arrayMap = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_arrayMap.js"(exports2, module2) {
    function arrayMap(array, iteratee) {
      var index = -1, length = array == null ? 0 : array.length, result = Array(length);
      while (++index < length) {
        result[index] = iteratee(array[index], index, array);
      }
      return result;
    }
    module2.exports = arrayMap;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_baseToString.js
var require_baseToString = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_baseToString.js"(exports2, module2) {
    var Symbol2 = require_Symbol();
    var arrayMap = require_arrayMap();
    var isArray = require_isArray();
    var isSymbol = require_isSymbol();
    var INFINITY = 1 / 0;
    var symbolProto = Symbol2 ? Symbol2.prototype : void 0;
    var symbolToString = symbolProto ? symbolProto.toString : void 0;
    function baseToString(value) {
      if (typeof value == "string") {
        return value;
      }
      if (isArray(value)) {
        return arrayMap(value, baseToString) + "";
      }
      if (isSymbol(value)) {
        return symbolToString ? symbolToString.call(value) : "";
      }
      var result = value + "";
      return result == "0" && 1 / value == -INFINITY ? "-0" : result;
    }
    module2.exports = baseToString;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/toString.js
var require_toString = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/toString.js"(exports2, module2) {
    var baseToString = require_baseToString();
    function toString(value) {
      return value == null ? "" : baseToString(value);
    }
    module2.exports = toString;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_castPath.js
var require_castPath = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_castPath.js"(exports2, module2) {
    var isArray = require_isArray();
    var isKey = require_isKey();
    var stringToPath = require_stringToPath();
    var toString = require_toString();
    function castPath(value, object) {
      if (isArray(value)) {
        return value;
      }
      return isKey(value, object) ? [value] : stringToPath(toString(value));
    }
    module2.exports = castPath;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_toKey.js
var require_toKey = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_toKey.js"(exports2, module2) {
    var isSymbol = require_isSymbol();
    var INFINITY = 1 / 0;
    function toKey(value) {
      if (typeof value == "string" || isSymbol(value)) {
        return value;
      }
      var result = value + "";
      return result == "0" && 1 / value == -INFINITY ? "-0" : result;
    }
    module2.exports = toKey;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_baseGet.js
var require_baseGet = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_baseGet.js"(exports2, module2) {
    var castPath = require_castPath();
    var toKey = require_toKey();
    function baseGet(object, path) {
      path = castPath(path, object);
      var index = 0, length = path.length;
      while (object != null && index < length) {
        object = object[toKey(path[index++])];
      }
      return index && index == length ? object : void 0;
    }
    module2.exports = baseGet;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/get.js
var require_get = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/get.js"(exports2, module2) {
    var baseGet = require_baseGet();
    function get2(object, path, defaultValue) {
      var result = object == null ? void 0 : baseGet(object, path);
      return result === void 0 ? defaultValue : result;
    }
    module2.exports = get2;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_copyArray.js
var require_copyArray = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/_copyArray.js"(exports2, module2) {
    function copyArray(source, array) {
      var index = -1, length = source.length;
      array || (array = Array(length));
      while (++index < length) {
        array[index] = source[index];
      }
      return array;
    }
    module2.exports = copyArray;
  }
});

// ../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/toPath.js
var require_toPath = __commonJS({
  "../../../.yarn/zpm/cache/lodash-npm-4.17.21-fee2decab8146c1171f1d28c9d98a4012c60fa79bd135040b476f388c9343670.zip/node_modules/lodash/toPath.js"(exports2, module2) {
    var arrayMap = require_arrayMap();
    var copyArray = require_copyArray();
    var isArray = require_isArray();
    var isSymbol = require_isSymbol();
    var stringToPath = require_stringToPath();
    var toKey = require_toKey();
    var toString = require_toString();
    function toPath2(value) {
      if (isArray(value)) {
        return arrayMap(value, toKey);
      }
      return isSymbol(value) ? [value] : copyArray(stringToPath(toString(value)));
    }
    module2.exports = toPath2;
  }
});

// index.ts
var index_exports = {};
module.exports = __toCommonJS(index_exports);
var import_get = __toESM(require_get());
var import_module = require("module");
var import_fs = require("fs");
var import_path = require("path");

// constraintsUtils.ts
var import_toPath = __toESM(require_toPath());

// miscUtils.ts
function getFactoryWithDefault(map, key, factory) {
  let value = map.get(key);
  if (typeof value === `undefined`)
    map.set(key, value = factory());
  return value;
}
function getArrayWithDefault(map, key) {
  let value = map.get(key);
  if (typeof value === `undefined`)
    map.set(key, value = []);
  return value;
}

// constraintsUtils.ts
var Index = class {
  constructor(indexedFields) {
    this.indexedFields = indexedFields;
    this.clear();
  }
  items = [];
  indexes = {};
  clear() {
    this.items = [];
    for (const field of this.indexedFields) {
      this.indexes[field] = /* @__PURE__ */ new Map();
    }
  }
  insert(item) {
    this.items.push(item);
    for (const field of this.indexedFields) {
      const value = Object.hasOwn(item, field) ? item[field] : void 0;
      if (typeof value === `undefined`)
        continue;
      const list = getArrayWithDefault(this.indexes[field], value);
      list.push(item);
    }
    return item;
  }
  find(filter) {
    if (typeof filter === `undefined`)
      return this.items;
    const filterEntries = Object.entries(filter);
    if (filterEntries.length === 0)
      return this.items;
    const sequentialFilters = [];
    let matches;
    for (const [field_, value] of filterEntries) {
      const field = field_;
      const index = Object.hasOwn(this.indexes, field) ? this.indexes[field] : void 0;
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
      result = result.filter((item) => {
        for (const [field, value] of sequentialFilters) {
          const valid = typeof value !== `undefined` ? Object.hasOwn(item, field) && item[field] === value : Object.hasOwn(item, field) === false;
          if (!valid) {
            return false;
          }
        }
        return true;
      });
    }
    return result;
  }
};
function normalizePath(p) {
  return Array.isArray(p) ? p : (0, import_toPath.default)(p);
}

// nodeUtils.ts
var chromeRe = /^\s*at (.*?) ?\(((?:file|https?|blob|chrome-extension|native|eval|webpack|<anonymous>|\/|[a-z]:\\|\\\\).*?)(?::(\d+))?(?::(\d+))?\)?\s*$/i;
var chromeEvalRe = /\((\S*)(?::(\d+))(?::(\d+))\)/;
function parseStackLine(line) {
  const parts = chromeRe.exec(line);
  if (!parts)
    return null;
  const isNative = parts[2] && parts[2].indexOf(`native`) === 0;
  const isEval = parts[2] && parts[2].indexOf(`eval`) === 0;
  const submatch = chromeEvalRe.exec(parts[2]);
  if (isEval && submatch != null) {
    parts[2] = submatch[1];
    parts[3] = submatch[2];
    parts[4] = submatch[3];
  }
  return {
    file: !isNative ? parts[2] : null,
    methodName: parts[1] || `<unknown>`,
    arguments: isNative ? [parts[2]] : [],
    line: parts[3] ? +parts[3] : null,
    column: parts[4] ? +parts[4] : null
  };
}
function getCaller() {
  const err = new Error();
  const line = err.stack.split(`
`)[3];
  return parseStackLine(line);
}

// index.ts
var allWorkspaceActions = /* @__PURE__ */ new Map();
var workspaceIndex = new Index([`cwd`, `ident`]);
var dependencyIndex = new Index([`workspace`, `type`, `ident`]);
var packageIndex = new Index([`ident`]);
var createSetFn = (workspaceCwd) => (path, value, { caller = getCaller() } = {}) => {
  const pathfieldPath = normalizePath(path);
  const key = pathfieldPath.join(`.`);
  const workspaceActions = getFactoryWithDefault(allWorkspaceActions, workspaceCwd, () => ({
    updates: /* @__PURE__ */ new Map(),
    errors: []
  }));
  const pathUpdates = getFactoryWithDefault(workspaceActions.updates, key, () => ({
    fieldPath: pathfieldPath,
    values: /* @__PURE__ */ new Map()
  }));
  const constraints = getFactoryWithDefault(pathUpdates.values, value, () => ({
    callers: []
  }));
  if (caller !== null) {
    constraints.callers.push(caller);
  }
};
var createErrorFn = (workspaceCwd) => (message) => {
  const workspaceActions = getFactoryWithDefault(allWorkspaceActions, workspaceCwd, () => ({
    updates: /* @__PURE__ */ new Map(),
    errors: []
  }));
  workspaceActions.errors.push({
    type: `userError`,
    message
  });
};
var RESULT_PATH = process.argv[2];
var input = JSON.parse(SERIALIZED_CONTEXT);
var packageByLocator = /* @__PURE__ */ new Map();
var workspaceByCwd = /* @__PURE__ */ new Map();
for (const workspace of input.workspaces) {
  const setFn = createSetFn(workspace.cwd);
  const errorFn = createErrorFn(workspace.cwd);
  const unsetFn = (path) => {
    return setFn(path, void 0, { caller: getCaller() });
  };
  const manifestPath = (0, import_path.join)(workspace.cwd, "package.json");
  const manifestContent = (0, import_fs.readFileSync)(manifestPath, "utf8");
  const manifest = JSON.parse(manifestContent);
  const hydratedWorkspace = {
    cwd: workspace.cwd,
    ident: workspace.ident,
    manifest,
    pkg: null,
    set: setFn,
    unset: unsetFn,
    error: errorFn
  };
  workspaceByCwd.set(workspace.cwd, hydratedWorkspace);
  workspaceIndex.insert(hydratedWorkspace);
}
for (const pkg of input.packages) {
  const workspace = pkg.workspace ? workspaceByCwd.get(pkg.workspace) : null;
  if (typeof workspace === "undefined")
    throw new Error(`Workspace ${pkg.workspace} not found`);
  const hydratedPackage = {
    ident: pkg.ident,
    workspace,
    version: pkg.version,
    dependencies: /* @__PURE__ */ new Map(),
    peerDependencies: new Map(pkg.peerDependencies),
    optionalPeerDependencies: new Map(pkg.optionalPeerDependencies)
  };
  packageByLocator.set(pkg.locator, hydratedPackage);
  packageIndex.insert(hydratedPackage);
}
for (const workspace of input.workspaces) {
  const setFn = createSetFn(workspace.cwd);
  const errorFn = createErrorFn(workspace.cwd);
  const hydratedWorkspace = workspaceByCwd.get(workspace.cwd);
  if (typeof hydratedWorkspace === "undefined")
    throw new Error(`Workspace ${workspace.cwd} not found`);
  for (const dependency of workspace.dependencies) {
    const resolution = dependency.resolution !== null ? packageByLocator.get(dependency.resolution) : null;
    if (typeof resolution === "undefined")
      throw new Error(`Dependency ${dependency.ident}@${dependency.range} (resolution: ${dependency.resolution}) not found`);
    const hydratedDependency = {
      workspace: hydratedWorkspace,
      ident: dependency.ident,
      range: dependency.range,
      type: dependency.dependencyType,
      resolution,
      update: (range) => {
        setFn([dependency.dependencyType, dependency.ident], range, { caller: getCaller() });
      },
      delete: () => {
        setFn([dependency.dependencyType, dependency.ident], void 0, { caller: getCaller() });
      },
      error: errorFn
    };
    dependencyIndex.insert(hydratedDependency);
  }
  for (const peerDependency of workspace.peerDependencies) {
    const hydratedPeerDependency = {
      workspace: hydratedWorkspace,
      ident: peerDependency.ident,
      range: peerDependency.range,
      type: `peerDependencies`,
      resolution: null,
      update: () => {
        setFn([`peerDependencies`, peerDependency.ident], peerDependency.range, { caller: getCaller() });
      },
      delete: () => {
        setFn([`peerDependencies`, peerDependency.ident], void 0, { caller: getCaller() });
      },
      error: errorFn
    };
    dependencyIndex.insert(hydratedPeerDependency);
  }
  for (const devDependency of workspace.devDependencies) {
    const resolution = devDependency.resolution !== null ? packageByLocator.get(devDependency.resolution) : null;
    if (typeof resolution === "undefined")
      throw new Error(`Dependency ${devDependency.ident} not found`);
    const hydratedDevDependency = {
      workspace: hydratedWorkspace,
      ident: devDependency.ident,
      range: devDependency.range,
      type: `devDependencies`,
      resolution,
      update: () => {
        setFn([`devDependencies`, devDependency.ident], devDependency.range, { caller: getCaller() });
      },
      delete: () => {
        setFn([`devDependencies`, devDependency.ident], void 0, { caller: getCaller() });
      },
      error: errorFn
    };
    dependencyIndex.insert(hydratedDevDependency);
  }
}
for (const pkg of input.packages) {
  const hydratedPackage = packageByLocator.get(pkg.locator);
  for (const [dependency, locator] of pkg.dependencies) {
    hydratedPackage.dependencies.set(dependency, packageByLocator.get(locator));
  }
}
var context = {
  Yarn: {
    workspace: (filter) => {
      return workspaceIndex.find(filter)[0] ?? null;
    },
    workspaces: (filter) => {
      return workspaceIndex.find(filter);
    },
    dependency: (filter) => {
      return dependencyIndex.find(filter)[0] ?? null;
    },
    dependencies: (filter) => {
      return dependencyIndex.find(filter);
    },
    package: (filter) => {
      return packageIndex.find(filter)[0] ?? null;
    },
    packages: (filter) => {
      return packageIndex.find(filter);
    }
  }
};
function applyEngineReport(fix) {
  const allWorkspaceOperations = /* @__PURE__ */ new Map();
  const allWorkspaceErrors = /* @__PURE__ */ new Map();
  for (const [workspaceCwd, workspaceActions] of allWorkspaceActions) {
    const manifest = workspaceByCwd.get(workspaceCwd).manifest;
    const workspaceErrors = workspaceActions.errors.slice();
    const workspaceOperations = [];
    for (const { fieldPath, values } of workspaceActions.updates.values()) {
      if (values.size > 1) {
        const valuesArray = [...values];
        const unsetValues = valuesArray.filter(([value]) => typeof value === `undefined`)?.[0]?.[1] ?? null;
        const setValues = valuesArray.filter(([value]) => typeof value !== `undefined`);
        workspaceErrors.push({
          type: `conflictingValues`,
          fieldPath,
          setValues,
          unsetValues
        });
      } else {
        const [[newValue]] = values;
        const currentValue = (0, import_get.default)(manifest, fieldPath);
        if (JSON.stringify(currentValue) === JSON.stringify(newValue))
          continue;
        if (!fix) {
          const error = typeof currentValue === `undefined` ? { type: `missingField`, fieldPath, expected: newValue } : typeof newValue === `undefined` ? { type: `extraneousField`, fieldPath, currentValue } : { type: `invalidField`, fieldPath, expected: newValue, currentValue };
          workspaceErrors.push(error);
          continue;
        }
        if (typeof newValue === `undefined`) {
          workspaceOperations.push({ type: `unset`, path: fieldPath });
        } else {
          workspaceOperations.push({ type: `set`, path: fieldPath, value: newValue });
        }
      }
    }
    if (workspaceOperations.length > 0) {
      allWorkspaceOperations.set(workspaceCwd, workspaceOperations);
    }
    if (workspaceErrors.length > 0) {
      allWorkspaceErrors.set(workspaceCwd, workspaceErrors);
    }
  }
  return {
    allWorkspaceOperations: [...allWorkspaceOperations],
    allWorkspaceErrors: [...allWorkspaceErrors]
  };
}
async function main() {
  const require2 = (0, import_module.createRequire)(CONFIG_PATH);
  const config = require2(CONFIG_PATH);
  await config.constraints?.(context);
  const output = applyEngineReport(FIX);
  (0, import_fs.writeFileSync)(RESULT_PATH, JSON.stringify(output, null, 2));
}
main();
