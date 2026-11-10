import 'package:json_annotation/json_annotation.dart';

part 'envelope.g.dart';

@JsonSerializable()
class BridgeFailure implements Exception {
  const BridgeFailure({required this.code, required this.message});

  final String code;
  final String message;

  factory BridgeFailure.fromJson(Map<String, dynamic> json) =>
      _$BridgeFailureFromJson(json);

  Map<String, dynamic> toJson() => _$BridgeFailureToJson(this);

  @override
  String toString() => message;
}

@JsonSerializable(explicitToJson: true)
class CompletionEnvelope {
  const CompletionEnvelope({
    required this.ok,
    this.value,
    this.error,
    this.closed = false,
    this.count,
  });

  final bool ok;
  final Object? value;
  final BridgeFailure? error;
  final bool closed;
  final String? count;

  factory CompletionEnvelope.fromJson(Map<String, dynamic> json) =>
      _$CompletionEnvelopeFromJson(json);

  Map<String, dynamic> toJson() => _$CompletionEnvelopeToJson(this);
}
