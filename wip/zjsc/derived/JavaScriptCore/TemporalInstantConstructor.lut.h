// Automatically generated from TemporalInstantConstructor.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex temporalInstantConstructorTableIndex[9] = {
    { 3, -1 },
    { 0, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { 1, 8 },
    { 2, -1 },
};

static constinit const struct HashTableValue temporalInstantConstructorTableValues[4] = {
   { "from"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, temporalInstantConstructorFuncFrom, 1 } },
   { "fromEpochMilliseconds"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, temporalInstantConstructorFuncFromEpochMilliseconds, 1 } },
   { "fromEpochNanoseconds"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, temporalInstantConstructorFuncFromEpochNanoseconds, 1 } },
   { "compare"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, temporalInstantConstructorFuncCompare, 2 } },
};

static constinit const struct HashTable temporalInstantConstructorTable =
    { 4, 7, false, nullptr, temporalInstantConstructorTableValues, temporalInstantConstructorTableIndex };

} // namespace JSC
