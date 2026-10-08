// Automatically generated from SymbolConstructor.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex symbolConstructorTableIndex[5] = {
    { 0, 4 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { 1, -1 },
};

static constinit const struct HashTableValue symbolConstructorTableValues[2] = {
   { "for"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, symbolConstructorFor, 1 } },
   { "keyFor"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, symbolConstructorKeyFor, 1 } },
};

static constinit const struct HashTable symbolConstructorTable =
    { 2, 3, false, nullptr, symbolConstructorTableValues, symbolConstructorTableIndex };

} // namespace JSC
