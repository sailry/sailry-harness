import '../../resources/file_page.dart';
import 'dart:convert';
import 'package:flutter/material.dart';

import '../../../content/code.dart';
import '../../../content/diff.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import 'references.dart';
import 'tool_display.dart';
import 'command_result.dart';

String toolLabel(String name, {Translator translate = tr}) =>
    hasLocalization('tool_$name') ? translate('tool_$name') : name;
String rawText(Object? value) =>
    value is String ? value : const JsonEncoder.withIndent('  ').convert(value);

/// Images and publications already occupy their existing answer rows.
/// Only sections rendered inside the tool disclosure need a body here.
bool hasToolContent(Map<String, dynamic> call) {
  if (call['failure'] != null) return true;
  final resolved = object(call['resolved']);
  if (objects(resolved['diagnostics']).isNotEmpty) return true;
  if (object(resolved['command']).isNotEmpty) return true;
  final output = object(call['output']);
  final body = object(output['body']);
  if (call['output_deferred'] == true ||
      text(output['preview']).isNotEmpty ||
      objects(output['notices']).isNotEmpty ||
      (output['paths'] is List && (output['paths'] as List).isNotEmpty) ||
      object(output['table']).isNotEmpty ||
      text(body['text']).isNotEmpty ||
      text(body['path']).isNotEmpty ||
      body['empty'] != null) {
    return true;
  }
  final texts = call['texts'] is List ? call['texts'] as List : const [];
  for (final (index, block) in objects(
    object(call['content'])['blocks'],
  ).indexed) {
    switch (block['kind']) {
      case 'text':
        if ((index < texts.length && text(texts[index]).isNotEmpty) ||
            objects(block['notices']).isNotEmpty) {
          return true;
        }
        break;
      case 'notice':
      case 'table':
      case 'diff':
        return true;
    }
  }
  return false;
}

class ToolContent extends StatelessWidget {
  const ToolContent({
    super.key,
    this.documents,
    required this.call,
    required this.result,
    required this.host,
    required this.worktree,
    this.output,
  });
  final Map<String, FileDocument>? documents;
  final Map<String, dynamic> call, result;
  final Map<String, dynamic>? output;
  final HostConnection host;
  final String worktree;

  @override
  Widget build(BuildContext context) {
    final muted = TextStyle(
      color: Theme.of(context).colorScheme.onSurfaceVariant,
    );
    final error = TextStyle(color: Theme.of(context).colorScheme.error);
    final value = result['result'];
    final returned = object(value);
    final resolved = object(call['resolved']);
    final command = object(resolved['command']);
    final projection = output ?? object(call['output']);
    final diagnostics = objects(resolved['diagnostics']);
    final observed = call['failure'] != null;
    final fault = observed ? returned['error'] : null;
    final failureTexts = observed
        ? objects(returned['content'])
              .where((item) => item['type'] == 'text')
              .map((item) => text(item['text']))
              .where((value) => value.trim().isNotEmpty)
              .toList()
        : const <String>[];
    final items = objects(object(call['content'])['blocks']);
    final texts = call['texts'] is List ? call['texts'] as List : const [];
    final structured =
        items.isNotEmpty &&
        fault == null &&
        diagnostics.isEmpty &&
        failureTexts.isEmpty;
    final paths = <String>{};
    final target = text(object(object(resolved['input'])['target'])['text']);
    if (target.isNotEmpty) paths.add(target);
    paths.addAll(
      (object(resolved['output'])['paths'] as List? ?? const [])
          .whereType<String>(),
    );
    paths.addAll(
      (projection['paths'] as List? ?? const []).whereType<String>(),
    );
    final blocks = <Widget>[];
    if (fault != null) {
      final message = fault is String ? fault : text(object(fault)['message']);
      if (message.isNotEmpty) {
        blocks.add(
          SelectableText(message, style: toolFailed(call) ? error : muted),
        );
      }
    } else if (diagnostics.isNotEmpty) {
      for (final item in diagnostics) {
        blocks.add(
          CodeBlock(
            text(item['text']),
            style: item['error'] == true ? error : muted,
          ),
        );
      }
    } else if (failureTexts.isNotEmpty) {
      for (final message in failureTexts) {
        blocks.add(
          SelectableText(message, style: toolFailed(call) ? error : muted),
        );
      }
    }
    for (final (index, block)
        in (structured ? items : <Map<String, dynamic>>[]).indexed) {
      if (block['kind'] == 'diff') {
        blocks.add(DiffView(key: ValueKey('diff-$index'), diff: block));
      }
      if (block['kind'] == 'table') blocks.add(_Table(block: block));
      if (block['kind'] == 'text') {
        final literal = index < texts.length ? text(texts[index]) : '';
        if (literal.isNotEmpty) {
          blocks.add(
            CodeBlock(literal, style: command.isNotEmpty ? muted : null),
          );
        }
        for (final notice in objects(block['notices'])) {
          blocks.add(Text(capturedLabel(context, notice)));
        }
      }
      if (block['kind'] == 'notice') {
        blocks.add(Text(capturedLabel(context, block['message'])));
      }
    }
    var projected = false;
    if (!structured && diagnostics.isEmpty && projection.isNotEmpty) {
      final body = object(projection['body']);
      final table = object(projection['table']);
      if (table.isNotEmpty) {
        blocks.add(_Table(block: table));
        projected = true;
      } else if (text(body['text']).isNotEmpty || body['empty'] != null) {
        final literal = text(body['text']);
        final path = text(body['path']);
        if (path.isNotEmpty) paths.add(path);
        if (body['diff'] != null) {
          blocks.add(
            DiffView(
              diff: {'path': path, 'text': literal, 'format': body['diff']},
            ),
          );
        } else if (literal.isNotEmpty) {
          blocks.add(CodeBlock(literal, language: languageFor(path)));
        } else {
          blocks.add(Text(capturedLabel(context, body['empty'])));
        }
        projected = true;
      } else {
        if (text(projection['preview']).isNotEmpty) {
          blocks.add(CodeBlock(text(projection['preview'])));
        }
        final notices = <String>{};
        for (final notice in objects(projection['notices'])) {
          final label = capturedLabel(context, notice);
          if (notices.add(label)) blocks.add(Text(label));
        }
        projected =
            blocks.isNotEmpty ||
            paths.isNotEmpty ||
            projection['preview_deferred'] == true;
      }
    }
    final commandLabel = commandStatus(command, translate: context.tr);
    if (commandLabel != null) {
      blocks.add(Text(commandLabel, style: toolFailed(call) ? error : muted));
      if (command['kind'] == 'unknown' && text(command['data']).isNotEmpty) {
        blocks.add(SelectableText(text(command['data']), style: muted));
      }
    }
    if (!structured && !projected && blocks.isEmpty && observed) {
      blocks.add(
        value == null
            ? Text(
                context.tr('toolFailed'),
                style: toolFailed(call) ? error : muted,
              )
            : CodeBlock(
                rawText(value),
                style: toolFailed(call) ? error : muted,
              ),
      );
    }
    String? copy;
    if (structured) {
      final literal = items.every(
        (block) => ['text', 'notice'].contains(block['kind']),
      );
      if (!literal) {
        copy = rawText(value);
      } else {
        final literals = <String>[
          for (final (index, block) in items.indexed)
            if (block['kind'] == 'text' &&
                index < texts.length &&
                text(texts[index]).isNotEmpty)
              text(texts[index]),
        ].join('\n');
        if (items.length > 1 ||
            items.any((block) => block['kind'] == 'notice') ||
            literals.isEmpty) {
          copy = literals.isNotEmpty
              ? literals
              : [
                  for (final block in items)
                    if (block['kind'] == 'notice')
                      capturedLabel(context, block['message'])
                    else
                      for (final notice in objects(block['notices']))
                        capturedLabel(context, notice),
                ].join('\n');
        }
      }
    } else if (projected) {
      if (object(projection['table']).isNotEmpty) {
        copy = rawText(value);
      } else if (text(projection['preview']).isEmpty &&
          objects(projection['notices']).isNotEmpty) {
        copy = objects(
          projection['notices'],
        ).map((notice) => capturedLabel(context, notice)).toSet().join('\n');
      }
    }
    return DefaultTextStyle.merge(
      style: muted,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (paths.isNotEmpty)
            MessageReferences(
              documents: documents,
              host: host,
              worktree: worktree,
              references: [
                for (final path in paths)
                  {
                    'label': path,
                    'target': {'kind': 'file', 'data': path},
                  },
              ],
            ),
          for (final block in blocks)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 4),
              child: block,
            ),
          if (copy != null && copy.isNotEmpty)
            Align(
              alignment: Alignment.centerRight,
              child: CopyTextButton(copy),
            ),
        ],
      ),
    );
  }
}

class _Table extends StatelessWidget {
  const _Table({required this.block});
  final Map<String, dynamic> block;
  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 360),
        child: SingleChildScrollView(
          child: SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: DataTable(
              columns: [
                for (final column in block['columns'] as List)
                  DataColumn(label: Text(column as String)),
              ],
              rows: [
                for (final row in block['rows'] as List)
                  DataRow(
                    cells: [
                      for (final cell in row as List)
                        DataCell(
                          SizedBox(
                            width: 180,
                            child: SelectableText(cell as String),
                          ),
                        ),
                    ],
                  ),
              ],
            ),
          ),
        ),
      ),
      if (block['truncated'] == true) Text(context.tr('resourcePartial')),
    ],
  );
}
