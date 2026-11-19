import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:sailry_bridge/api/commands.dart';
import 'package:sailry_bridge/bridge.dart';

import '../../../content/code.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import 'background_data.dart';

typedef OpenCommandDetail = Future<CommandDetail> Function();

class CommandDetailBody extends StatefulWidget {
  const CommandDetailBody({
    super.key,
    required this.open,
    required this.fallback,
    this.hold,
  });

  final OpenCommandDetail open;
  final Widget fallback;
  final String? hold;

  @override
  State<CommandDetailBody> createState() => _CommandDetailBodyState();
}

class _CommandDetailBodyState extends State<CommandDetailBody> {
  CommandDetail? _owner;
  late final _output = _read();

  Future<Map<String, dynamic>> _read() async {
    final owner = await widget.open();
    if (!mounted) {
      owner.dispose();
      return {};
    }
    _owner = owner;
    try {
      return object(jsonDecode(await owner.read()));
    } finally {
      if (identical(_owner, owner)) _release();
    }
  }

  void _release() {
    final owner = _owner;
    _owner = null;
    owner?.dispose();
  }

  @override
  void dispose() {
    _release();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => FutureBuilder<Map<String, dynamic>>(
    future: _output,
    builder: (context, snapshot) {
      if (snapshot.connectionState != ConnectionState.done) {
        return Text(context.tr('loading'));
      }
      final error = snapshot.error;
      if (error is BridgeFailure && error.code == 'permission_denied') {
        return Text(context.tr('failureDenied'));
      }
      final result = snapshot.data;
      if (result == null || result.isEmpty) return widget.fallback;
      final stdout = object(result['stdout']);
      final stderr = object(result['stderr']);
      final status = result['running'] == true
          ? context.tr('running')
          : context.tr(backgroundLabel(result['outcome']));
      final exit = result['exit_code'];
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        spacing: 8,
        children: [
          Text(exit is int && exit != 0 ? '$status ($exit)' : status),
          if (widget.hold case final hold?) Text(hold),
          if (result['failure'] case final String message
              when message.isNotEmpty)
            Text(message),
          if (stdout['text'] case final String text when text.isNotEmpty)
            CodeBlock(text),
          if (stderr['text'] case final String text when text.isNotEmpty)
            CodeBlock(text),
        ],
      );
    },
  );
}
