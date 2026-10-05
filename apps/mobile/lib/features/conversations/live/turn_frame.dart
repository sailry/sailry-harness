import 'dart:async';

import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import 'disclosure.dart';
import 'activity.dart';
import 'presentation.dart';

bool activeRun(Map<String, dynamic> run) =>
    ['queued', 'running', 'stopping'].contains(run['status']);

/// Execution timestamps come from the Node, never from widget mount time.
String? elapsed(Map<String, dynamic> run, int now) {
  final start = run['started_ms'] as int?;
  final end = activeRun(run) ? now : run['finished_ms'] as int?;
  if (start == null || end == null) return null;
  final seconds = ((end - start).clamp(0, double.maxFinite) / 1000).floor();
  final tail = (seconds % 60).toString().padLeft(2, '0');
  if (seconds < 60) return '${seconds}s';
  if (seconds < 3600) return '${seconds ~/ 60}m${tail}s';
  final minutes = (seconds ~/ 60 % 60).toString().padLeft(2, '0');
  return '${seconds ~/ 3600}h${minutes}m${tail}s';
}

class TurnFrame extends StatefulWidget {
  const TurnFrame({
    super.key,
    required this.run,
    required this.connected,
    required this.work,
    required this.answer,
    this.phase,
  });

  final Map<String, dynamic> run;
  final bool connected;
  final List<Widget> work;
  final List<Widget> answer;
  final String? phase;

  @override
  State<TurnFrame> createState() => _TurnFrameState();
}

class _TurnFrameState extends State<TurnFrame> {
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    _syncTimer();
  }

  @override
  void didUpdateWidget(covariant TurnFrame oldWidget) {
    super.didUpdateWidget(oldWidget);
    _syncTimer();
  }

  void _syncTimer() {
    final ticking =
        widget.connected &&
        activeRun(widget.run) &&
        widget.run['started_ms'] != null;
    if (ticking) {
      _timer ??= Timer.periodic(
        const Duration(seconds: 1),
        (_) => setState(() {}),
      );
    } else {
      _timer?.cancel();
      _timer = null;
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final active = activeRun(widget.run);
    final colors = Theme.of(context).colorScheme;
    final time = !active || widget.connected
        ? elapsed(widget.run, DateTime.now().millisecondsSinceEpoch)
        : null;
    final status = active && !widget.connected
        ? context.tr('conversationUnsynced')
        : widget.run['status'] == 'running'
        ? context.tr('conversationProcessing')
        : runLabel(widget.run['status'] as String?, translate: context.tr);
    final heading = Text(
      [status, ?time].join('  '),
      style: TextStyle(color: colors.onSurfaceVariant),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (widget.work.isNotEmpty)
          WorkDisclosure(
            key: ValueKey('work-${widget.run['turn']}'),
            title: heading,
            autoExpanded: active,
            framed: true,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: widget.work,
            ),
          )
        else
          Container(
            padding: const EdgeInsets.symmetric(vertical: 10),
            decoration: BoxDecoration(
              border: Border(bottom: BorderSide(color: colors.outlineVariant)),
            ),
            child: heading,
          ),
        const SizedBox(height: 12),
        ...widget.answer,
        if (active && widget.connected && widget.phase != null)
          Padding(
            padding: const EdgeInsets.only(bottom: 16),
            child: ActivityLabel(
              label: context.tr(widget.phase!),
              running: widget.phase != 'conversationWaiting',
              icon: 'spark',
              loadingIcon: true,
            ),
          ),
      ],
    );
  }
}
