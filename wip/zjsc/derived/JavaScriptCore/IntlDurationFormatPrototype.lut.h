// Automatically generated from IntlDurationFormatPrototype.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex durationFormatPrototypeTableIndex[8] = {
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { 2, -1 },
    { 0, -1 },
    { -1, -1 },
    { -1, -1 },
    { 1, -1 },
};

static constinit const struct HashTableValue durationFormatPrototypeTableValues[3] = {
   { "format"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, intlDurationFormatPrototypeFuncFormat, 1 } },
   { "formatToParts"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, intlDurationFormatPrototypeFuncFormatToParts, 1 } },
   { "resolvedOptions"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, intlDurationFormatPrototypeFuncResolvedOptions, 0 } },
};

static constinit const struct HashTable durationFormatPrototypeTable =
    { 3, 7, false, nullptr, durationFormatPrototypeTableValues, durationFormatPrototypeTableIndex };

} // namespace JSC
