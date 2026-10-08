// Automatically generated from WebAssemblyModuleConstructor.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex constructorTableWebAssemblyModuleIndex[8] = {
    { -1, -1 },
    { -1, -1 },
    { 2, -1 },
    { 1, -1 },
    { 0, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
};

static constinit const struct HashTableValue constructorTableWebAssemblyModuleValues[3] = {
   { "customSections"_s, static_cast<unsigned>(PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, webAssemblyModuleCustomSections, 2 } },
   { "imports"_s, static_cast<unsigned>(PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, webAssemblyModuleImports, 1 } },
   { "exports"_s, static_cast<unsigned>(PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, webAssemblyModuleExports, 1 } },
};

static constinit const struct HashTable constructorTableWebAssemblyModule =
    { 3, 7, false, nullptr, constructorTableWebAssemblyModuleValues, constructorTableWebAssemblyModuleIndex };

} // namespace JSC
