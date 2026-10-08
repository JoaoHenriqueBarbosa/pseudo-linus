// Automatically generated from TemporalPlainTimeConstructor.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex temporalPlainTimeConstructorTableIndex[4] = {
    { 1, -1 },
    { 0, -1 },
    { -1, -1 },
    { -1, -1 },
};

static constinit const struct HashTableValue temporalPlainTimeConstructorTableValues[2] = {
   { "from"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, temporalPlainTimeConstructorFuncFrom, 1 } },
   { "compare"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, temporalPlainTimeConstructorFuncCompare, 2 } },
};

static constinit const struct HashTable temporalPlainTimeConstructorTable =
    { 2, 3, false, nullptr, temporalPlainTimeConstructorTableValues, temporalPlainTimeConstructorTableIndex };

} // namespace JSC
