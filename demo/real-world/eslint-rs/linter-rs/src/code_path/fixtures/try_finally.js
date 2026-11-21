function g() {
    try { a(); return 1; } catch (e) { throw e; } finally { b(); }
    c();
}
try { d(); } finally { e(); }
try { f(); } catch { g(); }
