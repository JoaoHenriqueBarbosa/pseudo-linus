// Automatically generated from IntlObject.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex intlObjectTableIndex[33] = {
    { -1, -1 },
    { -1, -1 },
    { 0, -1 },
    { -1, -1 },
    { 4, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { 6, -1 },
    { 9, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { 2, -1 },
    { -1, -1 },
    { -1, -1 },
    { 11, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { 3, -1 },
    { 10, -1 },
    { -1, -1 },
    { 8, -1 },
    { 7, -1 },
    { -1, -1 },
    { -1, -1 },
    { 1, 32 },
    { 5, -1 },
};

static constinit const struct HashTableValue intlObjectTableValues[12] = {
   { "getCanonicalLocales"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, intlObjectFuncGetCanonicalLocales, 1 } },
   { "supportedValuesOf"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, intlObjectFuncSupportedValuesOf, 1 } },
   { "Collator"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::PropertyCallback), NoIntrinsic, { HashTableValue::LazyPropertyType, createCollatorConstructor } },
   { "DateTimeFormat"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::PropertyCallback), NoIntrinsic, { HashTableValue::LazyPropertyType, createDateTimeFormatConstructor } },
   { "DisplayNames"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::PropertyCallback), NoIntrinsic, { HashTableValue::LazyPropertyType, createDisplayNamesConstructor } },
   { "DurationFormat"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::PropertyCallback), NoIntrinsic, { HashTableValue::LazyPropertyType, createDurationFormatConstructor } },
   { "ListFormat"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::PropertyCallback), NoIntrinsic, { HashTableValue::LazyPropertyType, createListFormatConstructor } },
   { "Locale"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::PropertyCallback), NoIntrinsic, { HashTableValue::LazyPropertyType, createLocaleConstructor } },
   { "NumberFormat"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::PropertyCallback), NoIntrinsic, { HashTableValue::LazyPropertyType, createNumberFormatConstructor } },
   { "PluralRules"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::PropertyCallback), NoIntrinsic, { HashTableValue::LazyPropertyType, createPluralRulesConstructor } },
   { "RelativeTimeFormat"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::PropertyCallback), NoIntrinsic, { HashTableValue::LazyPropertyType, createRelativeTimeFormatConstructor } },
   { "Segmenter"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::PropertyCallback), NoIntrinsic, { HashTableValue::LazyPropertyType, createSegmenterConstructor } },
};

static constinit const struct HashTable intlObjectTable =
    { 12, 31, false, nullptr, intlObjectTableValues, intlObjectTableIndex };

} // namespace JSC
