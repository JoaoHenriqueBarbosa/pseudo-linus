// Automatically generated from SymbolPrototype.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex symbolPrototypeTableIndex[8] = {
    { 2, -1 },
    { -1, -1 },
    { -1, -1 },
    { 1, -1 },
    { -1, -1 },
    { 0, -1 },
    { -1, -1 },
    { -1, -1 },
};

static constinit const struct HashTableValue symbolPrototypeTableValues[3] = {
   { "description"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::ReadOnly|PropertyAttribute::CustomAccessor), NoIntrinsic, { HashTableValue::GetterSetterType, symbolProtoGetterDescription, 0 } },
   { "toString"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), SymbolPrototypeToStringIntrinsic, { HashTableValue::NativeFunctionType, symbolProtoFuncToString, 0 } },
   { "valueOf"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, symbolProtoFuncValueOf, 0 } },
};

static constinit const struct HashTable symbolPrototypeTable =
    { 3, 7, true, nullptr, symbolPrototypeTableValues, symbolPrototypeTableIndex };

} // namespace JSC
