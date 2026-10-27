import 'dart:async';
import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/toast.dart';
import 'presentation.dart';
import 'tool_display.dart';
import 'tool_output.dart';

/// Only the mounted publication surface requests the existing publication data.
class PublicationDetails extends StatefulWidget {
  const PublicationDetails({
    super.key,
    required this.calls,
    required this.enabled,
    required this.builder,
    this.load,
  });
  final List<Map<String, dynamic>> calls;
  final bool enabled;
  final OutputLoader? load;
  final Widget Function(BuildContext, List<Map<String, dynamic>>) builder;

  @override
  State<PublicationDetails> createState() => _PublicationDetailsState();
}

class _PublicationDetailsState extends State<PublicationDetails> {
  final _details = <Object, DeferredOutput>{};

  Object _key(Map<String, dynamic> call) => (
    object(call['source'])['entry'],
    object(call['source'])['index'],
    call['revision'],
  );

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _sync();
  }

  @override
  void didUpdateWidget(covariant PublicationDetails oldWidget) {
    super.didUpdateWidget(oldWidget);
    _sync();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  void _sync() {
    final calls = widget.calls.where((call) => call['published'] == true);
    final keys = calls.map(_key).toSet();
    for (final key
        in _details.keys.where((key) => !keys.contains(key)).toList()) {
      _details.remove(key)?.dispose();
    }
    if (!widget.enabled || !TickerMode.of(context)) return;
    for (final call in calls) {
      final key = _key(call);
      final detail = _details.putIfAbsent(key, () {
        return DeferredOutput()..addListener(_changed);
      });
      detail.bind(call);
      if (detail.failed) continue;
      unawaited(_load(key, detail, call));
    }
  }

  Future<void> _load(
    Object key,
    DeferredOutput detail,
    Map<String, dynamic> call, {
    bool retry = false,
  }) async {
    try {
      await detail.resolve(
        call,
        expanded: true,
        enabled: widget.enabled,
        load: widget.load,
        publication: true,
        retry: retry,
      );
    } catch (error) {
      if (mounted && identical(_details[key], detail)) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
    }
  }

  @override
  void dispose() {
    for (final detail in _details.values) {
      detail.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Column(
    mainAxisSize: MainAxisSize.min,
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      widget.builder(context, [
        for (final call in widget.calls)
          {...call, 'content': _details[_key(call)]?.value?['content']},
      ]),
      if (_details.values.any((detail) => detail.failed))
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton(
            onPressed: widget.enabled
                ? () {
                    for (final call in widget.calls) {
                      final key = _key(call);
                      final detail = _details[key];
                      if (detail != null && detail.failed) {
                        unawaited(_load(key, detail, call, retry: true));
                      }
                    }
                  }
                : null,
            child: Text(context.tr('retry')),
          ),
        ),
    ],
  );
}
