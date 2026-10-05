import '../../resources/file_page.dart';
import 'dart:convert';
import 'package:flutter/material.dart';

import '../../../content/code.dart';
import '../../../content/diff.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import 'command_result.dart';
import 'references.dart';
import 'tool_display.dart';

String toolLabel(String name, {Translator translate = tr}) =>
    hasLocalization('tool_$name') ? translate('tool_$name') : name;
String rawText(Object? value) =>
    value is String ? value : const JsonEncoder.withIndent('  ').convert(value);

class ToolContent extends StatelessWidget {
  const ToolContent({
    super.key,
    this.documents,
    required this.call,
    required this.result,
    required this.arguments,
    required this.host,
    required this.worktree,
  });
  final Map<String, FileDocument>? documents;
  final Map<String, dynamic> call, result, arguments;
  final HostConnection host;
  final String worktree;

  @override
  Widget build(BuildContext context) {
    final muted = TextStyle(
      color: Theme.of(context).colorScheme.onSurfaceVariant,
    );
    final value = result['result'];
    final output = object(value);
    final data = object(output['data']);
    final content = object(call['content']);
    final resolved = object(call['resolved']);
    final projection = object(resolved['output']);
    final diagnostics = objects(resolved['diagnostics']);
    final fault = output['error'];
    final hasFailure = resultFailed(output, resolved);
    final paths = <String>{};
    final target = text(object(object(resolved['input'])['target'])['text']);
    if (target.isNotEmpty) paths.add(target);
    if (projection['paths'] is List) {
      paths.addAll((projection['paths'] as List).whereType<String>());
    }
    if (resolved['label'] == null && arguments['path'] is String) {
      paths.add(arguments['path']);
    }
    if (output['kind'] == 'search_results') {
      for (final match in objects(data['matches'])) {
        if (match['path'] is String) paths.add(match['path']);
      }
    }
    final blocks = <Widget>[];
    if (fault != null) {
      final message = fault is String ? fault : text(object(fault)['message']);
      if (message.isNotEmpty) {
        blocks.add(
          SelectableText(
            message,
            style: hasFailure
                ? TextStyle(color: Theme.of(context).colorScheme.error)
                : muted,
          ),
        );
      }
    } else if (diagnostics.isNotEmpty) {
      for (final item in diagnostics) {
        blocks.add(
          CodeBlock(
            text(item['text']),
            style: item['error'] == true
                ? TextStyle(color: Theme.of(context).colorScheme.error)
                : muted,
          ),
        );
      }
    } else if (hasFailure) {
      final failure = output['isError'] == true
          ? output
          : object(output['output']);
      for (final item in objects(failure['content'])) {
        if (item['type'] == 'text') {
          blocks.add(
            SelectableText(
              text(item['text']),
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          );
        }
      }
    }
    for (final (index, block) in objects(content['blocks']).indexed) {
      if (block['kind'] == 'diff') {
        blocks.add(DiffView(key: ValueKey('diff-$index'), diff: block));
      }
      if (block['kind'] == 'table') blocks.add(_Table(block: block));
      if (block['kind'] == 'text') {
        final payload = output.containsKey('sailry_content')
            ? output
            : object(output['output']);
        final literal = contentValue(payload, text(block['path']));
        if (literal is String) blocks.add(CodeBlock(literal));
        for (final notice in objects(block['notices'])) {
          blocks.add(Text(capturedLabel(context, notice)));
        }
      }
      if (block['kind'] == 'notice') {
        blocks.add(Text(capturedLabel(context, block['message'])));
      }
      if (block['kind'] == 'file') paths.add(text(block['path']));
      // Authenticated result images are rendered once in the answer by LiveTimeline.
    }
    var projected = false;
    if (content.isEmpty &&
        !hasFailure &&
        fault == null &&
        diagnostics.isEmpty &&
        projection.isNotEmpty) {
      final body = object(projection['body']);
      final table = object(projection['table']);
      if (table.isNotEmpty) {
        blocks.add(_Table(block: table));
        projected = true;
      } else if (body.isNotEmpty) {
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
        } else if (body['empty'] != null) {
          blocks.add(Text(capturedLabel(context, body['empty'])));
        }
        projected = true;
      } else if (text(projection['preview']).isNotEmpty ||
          objects(projection['notices']).isNotEmpty) {
        if (text(projection['preview']).isNotEmpty) {
          blocks.add(CodeBlock(text(projection['preview'])));
        }
        final notices = <String>{};
        for (final notice in objects(projection['notices'])) {
          final label = capturedLabel(context, notice);
          if (notices.add(label)) blocks.add(Text(label));
        }
        projected = true;
      } else if (paths.isNotEmpty) {
        projected = true;
      }
    }
    if (content.isEmpty && !projected && blocks.isEmpty) {
      if (output['kind'] == 'file_content') {
        blocks.add(
          CodeBlock(
            text(data['text']),
            style: muted,
            language: languageFor(text(arguments['path'])),
          ),
        );
      } else if (call['name'] == 'write_file' &&
          output['kind'] == 'file_written') {
        blocks.add(
          CodeBlock(
            text(arguments['text']),
            style: muted,
            language: languageFor(text(arguments['path'])),
          ),
        );
      } else if (data['stdout'] != null || data['stderr'] != null) {
        for (final key in ['stdout', 'stderr']) {
          final capture = object(data[key]);
          if (text(capture['text']).isNotEmpty) {
            blocks.add(
              CodeBlock(text(capture['text']), title: key, style: muted),
            );
          }
          if (capture['truncated'] == true) {
            blocks.add(Text(context.tr('resourcePartial')));
          }
        }
        final outcome = commandOutcome(output);
        final status = commandStatus(output, translate: context.tr);
        if (status != null) {
          blocks.add(
            Text(
              status,
              style: commandFailed(output)
                  ? TextStyle(color: Theme.of(context).colorScheme.error)
                  : muted,
            ),
          );
        }
        if (outcome['kind'] == 'unknown') {
          blocks.add(SelectableText(text(outcome['data']), style: muted));
        }
      } else if (output['kind'] == 'search_results') {
        for (final match in objects(data['matches'])) {
          blocks.add(
            SelectableText(
              '${match['path']}:${match['line_number']}  ${text(match['line'])}',
            ),
          );
        }
      } else if (output['kind'] == 'directory') {
        for (final entry in objects(data['entries'])) {
          blocks.add(
            Text(
              '${text(entry['name'])}${entry['kind'] == 'directory' ? '/' : ''}',
            ),
          );
        }
      } else {
        blocks.add(
          CodeBlock(
            rawText(value),
            language: value is String ? null : 'json',
            style: muted,
          ),
        );
      }
      if (data['truncated'] == true) {
        blocks.add(Text(context.tr('resourcePartial')));
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
                    'target': {
                      'kind': output['kind'] == 'directory'
                          ? 'directory'
                          : 'file',
                      'data': path,
                    },
                  },
              ],
            ),
          for (final block in blocks)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 4),
              child: block,
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
