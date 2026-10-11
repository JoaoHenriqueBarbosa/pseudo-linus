// Automatically generated from TemporalNow.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex temporalNowTableIndex[17] = {
    { 5, -1 },
    { 2, -1 },
    { 3, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { 0, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { 1, 16 },
    { -1, -1 },
    { 4, -1 },
};

static constinit const struct HashTableValue temporalNowTableValues[6] = {
   { "instant"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, temporalNowFuncInstant, 0 } },
   { "timeZoneId"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, temporalNowFuncTimeZoneId, 0 } },
   { "plainDateISO"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, temporalNowFuncPlainDateISO, 0 } },
   { "plainDateTimeISO"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, temporalNowFuncPlainDateTimeISO, 0 } },
   { "plainTimeISO"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, temporalNowFuncPlainTimeISO, 0 } },
   { "zonedDateTimeISO"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, temporalNowFuncZonedDateTimeISO, 0 } },
};

static constinit const struct HashTable temporalNowTable =
    { 6, 15, false, nullptr, temporalNowTableValues, temporalNowTableIndex };

} // namespace JSC
