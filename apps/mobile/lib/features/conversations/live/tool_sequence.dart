import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'disclosure.dart';
import 'tool_record.dart';
import 'tool_heading.dart';

/// Groups consecutive activity without changing the shared Client's calls.
class ToolSequence extends StatefulWidget {
  const ToolSequence({super.key, required this.tools});

  final List<ToolRecord> tools;

  @override
  State<ToolSequence> createState() => _ToolSequenceState();
}

class _ToolSequenceState extends State<ToolSequence> {
  final _scroll = ScrollController();

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tools = widget.tools;
    final pending = tools.any((tool) => pendingTool(tool.call));
    final running = tools.any(
      (tool) => tool.continuing || tool.call['state'] == 'running',
    );
    final latest =
        tools
            .where(
              (tool) =>
                  pendingTool(tool.call) ||
                  tool.continuing ||
                  tool.call['state'] == 'running',
            )
            .lastOrNull ??
        tools.last;
    return WorkDisclosure(
      autoExpanded: false,
      title: ToolHeading(
        call: latest.call,
        page: latest.page,
        pending: pending,
        running: running,
        failed: tools.any((tool) => toolFailed(tool.call, tool.page)),
      ),
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxHeight: math.min(320, MediaQuery.sizeOf(context).height * .5),
        ),
        child: Scrollbar(
          controller: _scroll,
          thumbVisibility: true,
          child: SingleChildScrollView(
            key: PageStorageKey(widget.key),
            controller: _scroll,
            primary: false,
            padding: const EdgeInsets.only(right: 12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: tools,
            ),
          ),
        ),
      ),
    );
  }
}
