import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/runtime/session.dart';

class ContextHost extends HostConnection {
  ContextHost(String id, {super.connected = true})
    : super.test(id: id, label: id, snapshot: catalog(id));

  static Map<String, dynamic> catalog(String id) => {
    'projects': [
      {'id': 'same-project-id', 'name': '$id project'},
    ],
    'worktrees': [
      {'id': '$id-main', 'project': 'same-project-id', 'main': true},
      {'id': '$id-feature', 'project': 'same-project-id'},
      {'id': '$id-scratch', 'project': null},
    ],
  };

  void publish(Map<String, dynamic> value) {
    snapshot = value;
    notifyListeners();
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('host project and worktree switch as one global context', () async {
    final first = ContextHost('first');
    final second = ContextHost('second', connected: false);
    final session = AppSession.test(hosts: [first, second]);
    final observed = <(String?, String?, String?)>[];
    session.addListener(
      () => observed.add((
        session.selectedHost?.id,
        session.selectedProjectId,
        session.selectedWorktreeId,
      )),
    );

    expect(session.selectedProjectId, isNull);
    expect(session.selectedWorktreeId, isNull);
    session.selectWorkspace(first.id, 'first-feature');
    expect(observed, [('first', 'same-project-id', 'first-feature')]);
    session.selectHost(first.id);
    expect(observed.length, 1);
    expect(session.selectedWorktreeId, 'first-feature');

    // Equal project IDs on different hosts must not retain another host's tree.
    session.selectProject(second.id, 'same-project-id');
    expect(observed.length, 2);
    expect(observed.last, ('second', 'same-project-id', 'second-main'));
    expect(session.selectedHost?.connected, isFalse);

    session.selectHost(first.id);
    expect(observed.last, ('first', null, null));
    session.selectProject(first.id, '');
    expect(observed.last, ('first', '', null));
    session.selectProject(first.id, null);
    expect(observed.last, ('first', null, null));

    final notifications = observed.length;
    session.selectProject(second.id, 'missing');
    session.selectWorkspace(second.id, 'second-scratch');
    session.selectWorkspace(second.id, 'first-main');
    session.selectHost('missing');
    expect(observed.length, notifications);
    expect(session.selectedHost, same(first));
    await session.close();
    session.dispose();
  });

  test(
    'global project follows its own catalog without picking another',
    () async {
      final host = ContextHost('host');
      final session = AppSession.test(hosts: [host]);
      session.selectWorkspace(host.id, 'host-feature');
      host.publish({
        'projects': ContextHost.catalog(host.id)['projects'],
        'worktrees': <Map<String, dynamic>>[],
      });
      expect(session.selectedProjectId, 'same-project-id');
      expect(session.selectedWorktreeId, isNull);
      host.publish(ContextHost.catalog(host.id));
      expect(session.selectedWorktreeId, 'host-feature');
      host.publish({
        'projects': [
          {'id': 'replacement', 'name': 'Other project'},
        ],
        'worktrees': [
          {'id': 'replacement-tree', 'project': 'replacement', 'main': true},
        ],
      });
      expect(session.selectedProjectId, isNull);
      expect(session.selectedWorktreeId, isNull);
      await session.close();
      session.dispose();
    },
  );
}
