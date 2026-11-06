import 'dart:async';

import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import 'disclosure.dart';
import 'tool_row.dart';

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

/// Summarises a turn's steps above its answer and keeps the live step below it.
class TurnFrame extends StatefulWidget {
  const TurnFrame({
    super.key,
    required this.run,
    required this.connected,
    required this.steps,
    required this.count,
    required this.answer,
    this.current,
    this.phase,
    this.segment = 0,
    this.settled = false,
  });

  final Map<String, dynamic> run;
  final bool connected;
  final List<Widget> steps;

  /// Tool calls among [steps]; without any, the steps stay inline.
  final int count;
  final List<Widget> answer;
  final Widget? current;
  final String? phase;

  /// Position within the turn when delivered input divides its reply.
  final int segment;

  /// An earlier segment shows neither the turn's timing nor its live step.
  final bool settled;

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
        !widget.settled &&
        widget.connected &&
        activeRun(widget.run) &&
        widget.run['started_ms'] != null &&
        widget.count > 0;
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
    final live = active && widget.connected && !widget.settled;
    final current =
        widget.current ??
        (widget.phase == null
            ? null
            : ToolRow(
                icon: 'spark',
                label: context.tr(widget.phase!),
                running: widget.phase != 'conversationWaiting',
              ));
    final turn = widget.run['turn'];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (widget.count > 0) ...[
          StepDisclosure(
            key: ValueKey(
              widget.segment == 0
                  ? 'work-$turn'
                  : 'work-$turn-${widget.segment}',
            ),
            label: widget.count == 1
                ? context.tr('conversationStep')
                : context
                      .tr('conversationSteps')
                      .replaceAll('{count}', '${widget.count}'),
            detail: widget.settled
                ? null
                : !active || widget.connected
                ? elapsed(widget.run, DateTime.now().millisecondsSinceEpoch)
                : null,
            children: widget.steps,
          ),
          const SizedBox(height: 10),
        ] else
          ...widget.steps,
        ...widget.answer,
        if (live && current != null)
          Padding(padding: const EdgeInsets.only(bottom: 16), child: current),
      ],
    );
  }
}
