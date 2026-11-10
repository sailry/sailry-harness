// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'envelope.dart';

// **************************************************************************
// JsonSerializableGenerator
// **************************************************************************

BridgeFailure _$BridgeFailureFromJson(Map<String, dynamic> json) =>
    BridgeFailure(
      code: json['code'] as String,
      message: json['message'] as String,
    );

Map<String, dynamic> _$BridgeFailureToJson(BridgeFailure instance) =>
    <String, dynamic>{'code': instance.code, 'message': instance.message};

CompletionEnvelope _$CompletionEnvelopeFromJson(Map<String, dynamic> json) =>
    CompletionEnvelope(
      ok: json['ok'] as bool,
      value: json['value'],
      error: json['error'] == null
          ? null
          : BridgeFailure.fromJson(json['error'] as Map<String, dynamic>),
      closed: json['closed'] as bool? ?? false,
      count: json['count'] as String?,
    );

Map<String, dynamic> _$CompletionEnvelopeToJson(CompletionEnvelope instance) =>
    <String, dynamic>{
      'ok': instance.ok,
      'value': instance.value,
      'error': instance.error?.toJson(),
      'closed': instance.closed,
      'count': instance.count,
    };
