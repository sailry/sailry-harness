import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import 'resources_page.dart';
import 'workspace.dart';
import '../../runtime/session.dart';
import 'live_hosts.dart';

class HostsPage extends StatefulWidget {
  const HostsPage({super.key, this.onSettings, this.onBrowse});

  final ValueChanged<String>? onSettings;
  final ValueChanged<String>? onBrowse;

  @override
  State<HostsPage> createState() => _HostsPageState();
}

class _HostsPageState extends State<HostsPage> {
  String _host = 'Studio';
  bool _paired = false;

  bool get _offline => _host == 'MacBook Air';
  bool get _server => _host == 'Build Server';

  Future<void> _pair() async {
    final added = await showAppSheet<bool>(
      context,
      context.tr('pairTitle'),
      child: const _PairForm(),
    );
    if (added == true && mounted) {
      setState(() {
        _paired = true;
        _host = 'Preview Host';
      });
      showResourceNotice(context, 'pairSuccess');
    }
  }

  void _details() => showAppSheet<void>(
    context,
    context.tr('manageHost'),
    child: Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        ListTile(
          contentPadding: EdgeInsets.zero,
          leading: const AppIcon('link'),
          title: Text(context.tr('connection')),
          trailing: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(context.tr(_offline ? 'offline' : 'direct')),
              const AppIcon('chevron'),
            ],
          ),
          onTap: () {
            Navigator.pop(context);
            showAppSheet<void>(
              context,
              context.tr('connectionDetails'),
              child: Surface(
                child: Text(
                  _offline
                      ? context.tr('offline')
                      : '${context.tr('direct')}\n${context.tr('latency')}: ${_server ? 32 : 8} ms',
                ),
              ),
            );
          },
        ),
        const Divider(),
        ListTile(
          contentPadding: EdgeInsets.zero,
          leading: const AppIcon('folder'),
          title: Text(context.tr('hostProjects')),
          trailing: const AppIcon('chevron'),
          onTap: () {
            Navigator.pop(context);
            pushPage(
              context,
              ResourcesPage(
                host: _host,
                project: _server ? 'sailry-api' : 'sailry-web',
              ),
            );
          },
        ),
        const Divider(),
        ListTile(
          contentPadding: EdgeInsets.zero,
          leading: const AppIcon('settings'),
          title: Text(context.tr('nodeSettings')),
          trailing: const AppIcon('chevron'),
          onTap: () {
            Navigator.pop(context);
            widget.onSettings?.call(_host);
          },
        ),
      ],
    ),
  );

  @override
  Widget build(BuildContext context) {
    if (AppSession.maybeOf(context) != null) {
      return LiveHostsPage(
        onSettings: widget.onSettings,
        onBrowse: widget.onBrowse,
      );
    }
    final colors = Theme.of(context).colorScheme;
    final metrics = _server ? [68, 42, 19] : [24, 58, 36];
    final counts = _server ? [2, 1, 2] : [4, 2, 3];
    return PageFrame(
      title: context.tr('hosts'),
      actions: [
        RoundButton(
          icon: 'server',
          tooltip: context.tr('selectHost'),
          onPressed: () => showHostPicker(
            context,
            selected: _host,
            additionalHosts: _paired ? ['Preview Host'] : [],
            onAdd: _pair,
            onSelected: (host) => setState(() => _host = host),
          ),
        ),
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Surface(
            onTap: _details,
            child: Column(
              children: [
                Row(
                  children: [
                    Surface(
                      radius: 14,
                      child: AppIcon(_server ? 'server' : 'laptop', size: 26),
                    ),
                    const SizedBox(width: 12),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            _host,
                            style: Theme.of(context).textTheme.titleLarge,
                          ),
                          const SizedBox(height: 4),
                          Text(
                            context.tr(
                              _offline
                                  ? 'laptopSystem'
                                  : _server
                                  ? 'serverSystem'
                                  : 'hostSystem',
                            ),
                            style: TextStyle(color: colors.onSurfaceVariant),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 16),
                const Divider(),
                const SizedBox(height: 8),
                Row(
                  children: [
                    _Dot(
                      color: _offline
                          ? colors.onSurfaceVariant
                          : colors.tertiary,
                    ),
                    const SizedBox(width: 6),
                    Expanded(
                      child: Text(
                        context.tr(_offline ? 'offline' : 'statusHealthy'),
                        style: TextStyle(color: colors.onSurfaceVariant),
                      ),
                    ),
                    Text(
                      _offline
                          ? '—'
                          : '${context.tr('direct')} · ${_server ? 32 : 8} ms',
                      style: TextStyle(color: colors.onSurfaceVariant),
                    ),
                  ],
                ),
              ],
            ),
          ),
          const SizedBox(height: 12),
          if (_offline)
            Surface(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(context.tr('hostOffline')),
                  const SizedBox(height: 16),
                  OutlinedButton(
                    onPressed: () => showResourceNotice(context, 'retryNote'),
                    child: Text(context.tr('retry')),
                  ),
                ],
              ),
            )
          else ...[
            Surface(
              child: Column(
                children: [
                  Row(
                    children: [
                      for (var index = 0; index < metrics.length; index++)
                        Expanded(
                          child: _Metric(
                            value: metrics[index],
                            label: context.tr(['cpu', 'memory', 'disk'][index]),
                          ),
                        ),
                    ],
                  ),
                  const SizedBox(height: 18),
                  const Divider(),
                  const SizedBox(height: 8),
                  Row(
                    children: [
                      for (var index = 0; index < counts.length; index++)
                        Expanded(
                          child: Column(
                            children: [
                              Text(
                                '${counts[index]}',
                                style: Theme.of(context).textTheme.titleMedium,
                              ),
                              const SizedBox(height: 3),
                              Text(
                                context.tr(
                                  [
                                    'sessionCount',
                                    'terminalCount',
                                    'projectCount',
                                  ][index],
                                ),
                                style: TextStyle(
                                  color: colors.onSurfaceVariant,
                                ),
                              ),
                            ],
                          ),
                        ),
                    ],
                  ),
                ],
              ),
            ),
            const SizedBox(height: 12),
            Surface(
              child: Column(
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          context.tr('activity'),
                          style: Theme.of(context).textTheme.titleSmall,
                        ),
                      ),
                      Text(
                        context.tr('lastHour'),
                        style: TextStyle(color: colors.onSurfaceVariant),
                      ),
                    ],
                  ),
                  const SizedBox(height: 20),
                  Semantics(
                    label:
                        '${context.tr('activity')} · ${context.tr('sample')}',
                    child: SizedBox(
                      height: 76,
                      child: BarChart(
                        BarChartData(
                          maxY: 70,
                          titlesData: const FlTitlesData(show: false),
                          gridData: const FlGridData(show: false),
                          borderData: FlBorderData(show: false),
                          barTouchData: BarTouchData(enabled: false),
                          barGroups: [
                            for (var index = 0; index < 34; index++)
                              BarChartGroupData(
                                x: index,
                                barRods: [
                                  BarChartRodData(
                                    toY:
                                        (14 +
                                                ((index * 17 +
                                                        (_server ? 23 : 0)) %
                                                    54))
                                            .toDouble(),
                                    width: 4,
                                    color: index % 9 == 0
                                        ? colors.secondary
                                        : index > 12 && index % 5 != 0
                                        ? colors.tertiary
                                        : colors.outline,
                                    borderRadius: BorderRadius.circular(2),
                                  ),
                                ],
                              ),
                          ],
                        ),
                        duration: Duration.zero,
                      ),
                    ),
                  ),
                  const SizedBox(height: 12),
                  DefaultTextStyle(
                    style: Theme.of(context).textTheme.bodySmall!.copyWith(
                      color: colors.onSurfaceVariant,
                    ),
                    child: const Row(
                      mainAxisAlignment: MainAxisAlignment.spaceBetween,
                      children: [Text('08:40'), Text('09:10'), Text('09:40')],
                    ),
                  ),
                  const SizedBox(height: 14),
                  Row(
                    children: [
                      _Dot(color: colors.tertiary),
                      const SizedBox(width: 6),
                      Text(context.tr('running')),
                      const SizedBox(width: 14),
                      _Dot(color: colors.secondary),
                      const SizedBox(width: 6),
                      Text(context.tr('waiting')),
                    ],
                  ),
                ],
              ),
            ),
            const SizedBox(height: 12),
            Surface(
              child: Table(
                columnWidths: const {
                  0: FlexColumnWidth(2),
                  1: FlexColumnWidth(),
                  2: FlexColumnWidth(),
                },
                defaultVerticalAlignment: TableCellVerticalAlignment.middle,
                children: [
                  for (final row in [
                    [context.tr('process'), 'CPU', context.tr('memory')],
                    [
                      _server ? 'sailry-host' : 'sailry-desktop',
                      '12.4%',
                      '486 MB',
                    ],
                    ['node', '6.8%', '218 MB'],
                    ['cargo', '4.2%', '162 MB'],
                  ])
                    TableRow(
                      children: [
                        for (final value in row)
                          Padding(
                            padding: const EdgeInsets.symmetric(vertical: 8),
                            child: Text(
                              value,
                              style: Theme.of(context).textTheme.bodySmall,
                            ),
                          ),
                      ],
                    ),
                ],
              ),
            ),
          ],
        ],
      ),
    );
  }
}

class _PairForm extends StatefulWidget {
  const _PairForm();

  @override
  State<_PairForm> createState() => _PairFormState();
}

class _PairFormState extends State<_PairForm> {
  final _form = GlobalKey<FormState>();

  @override
  Widget build(BuildContext context) => Form(
    key: _form,
    child: Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(context.tr('pairDescription')),
        const SizedBox(height: 16),
        TextFormField(
          autofocus: true,
          keyboardType: TextInputType.number,
          maxLength: 6,
          decoration: InputDecoration(
            labelText: context.tr('pairCode'),
            hintText: '000000',
          ),
          validator: (value) => RegExp(r'^\d{6}$').hasMatch(value ?? '')
              ? null
              : context.tr('pairInvalid'),
        ),
        const SizedBox(height: 12),
        Text(
          context.tr('pairHint'),
          style: Theme.of(context).textTheme.bodySmall,
        ),
        const SizedBox(height: 16),
        FilledButton(
          onPressed: () {
            if (_form.currentState!.validate()) Navigator.pop(context, true);
          },
          child: Text(context.tr('pairDemo')),
        ),
      ],
    ),
  );
}

class _Metric extends StatelessWidget {
  const _Metric({required this.value, required this.label});
  final int value;
  final String label;

  @override
  Widget build(BuildContext context) => Column(
    children: [
      SizedBox.square(
        dimension: 68,
        child: Stack(
          alignment: Alignment.center,
          children: [
            SizedBox.expand(
              child: CircularProgressIndicator(
                value: value / 100,
                strokeWidth: 4,
                color: Theme.of(context).colorScheme.tertiary,
                backgroundColor: Theme.of(context).colorScheme.outline,
                semanticsLabel: label,
                semanticsValue: '$value%',
              ),
            ),
            Text('$value%', style: Theme.of(context).textTheme.titleMedium),
          ],
        ),
      ),
      const SizedBox(height: 12),
      Text(
        label,
        style: TextStyle(color: Theme.of(context).colorScheme.onSurfaceVariant),
      ),
    ],
  );
}

class _Dot extends StatelessWidget {
  const _Dot({required this.color});
  final Color color;

  @override
  Widget build(BuildContext context) => Container(
    width: 6,
    height: 6,
    decoration: BoxDecoration(color: color, shape: BoxShape.circle),
  );
}
