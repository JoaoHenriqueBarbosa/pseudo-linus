// Automatically generated from IntlSegmenterPrototype.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex segmenterPrototypeTableIndex[4] = {
    { -1, -1 },
    { 0, -1 },
    { -1, -1 },
    { 1, -1 },
};

static constinit const struct HashTableValue segmenterPrototypeTableValues[2] = {
   { "segment"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, intlSegmenterPrototypeFuncSegment, 1 } },
   { "resolvedOptions"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, intlSegmenterPrototypeFuncResolvedOptions, 0 } },
};

static constinit const struct HashTable segmenterPrototypeTable =
    { 2, 3, false, nullptr, segmenterPrototypeTableValues, segmenterPrototypeTableIndex };

} // namespace JSC
