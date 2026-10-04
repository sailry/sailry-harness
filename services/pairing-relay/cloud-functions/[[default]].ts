import handle from "../src/edgeone";

export default function onRequest(context: Parameters<typeof handle>[0]) { return handle(context); }
