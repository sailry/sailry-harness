import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import 'tool_display.dart';
import 'tool_output.dart';

/// Published references are retained only while this surface is mounted.
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
  final ToolSelectionChanged? load;
  final Widget Function(BuildContext, List<Map<String, dynamic>>) builder;

  @override
  State<PublicationDetails> createState() => _PublicationDetailsState();
}

class _PublicationDetailsState extends State<PublicationDetails> {
  final _selection = Object();

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

  void _sync({bool retry = false}) {
    widget.load?.call(
      _selection,
      widget.enabled && TickerMode.valuesOf(context).enabled
          ? widget.calls.where((call) => call['published'] == true).toList()
          : [],
      retry,
    );
  }

  @override
  void dispose() {
    widget.load?.call(_selection, [], false);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Column(
    mainAxisSize: MainAxisSize.min,
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      widget.builder(context, [
        for (final call in widget.calls)
          {...call, 'content': toolDetail(call)?['content']},
      ]),
      if (widget.calls.any(detailFailed))
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton(
            onPressed: widget.enabled ? () => _sync(retry: true) : null,
            child: Text(context.tr('retry')),
          ),
        ),
    ],
  );
}
