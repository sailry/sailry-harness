// Host Flutter acceptance uses the same real-network flow as device execution.
import '../integration_test/node_test.dart' as acceptance;

void main() => acceptance.main(host: true);
