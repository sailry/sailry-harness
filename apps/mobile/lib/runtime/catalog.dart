import 'json.dart';
import 'session.dart';

/// A resource keeps its execution host when projected into a combined list.
typedef HostRecord = (HostConnection host, Map<String, dynamic> value);

/// Retains cached records from offline hosts. Resource IDs are local to a Node,
/// so equal IDs on different hosts remain separate resources.
List<HostRecord> hostRecords(
  Iterable<HostConnection> hosts,
  String collection,
) {
  final seen = <(String, String)>{};
  return [
    for (final host in hosts)
      for (final value in objects(host.snapshot[collection]))
        if (seen.add((host.id, text(value['id'])))) (host, value),
  ];
}
