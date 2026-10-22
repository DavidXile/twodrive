import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:flutter/foundation.dart';
import 'updates.dart';

typedef Json = Map<String, dynamic>;

class Transfer {
  Transfer(this.id, this.operation, this.args);
  final int id;
  final String operation;
  final Json args;
  int done = 0;
  int total = 0;
  String state = '传输中';
  String? error;
  String get name => (args['path'] as String).split('/').last;
}

class StorageClient extends ChangeNotifier {
  StorageClient() {
    updates.addListener(notifyListeners);
  }
  final updates = UpdateChecker();
  Process? _process;
  final Map<int, Completer<Json>> _pending = {};
  final List<Transfer> transfers = [];
  List<Json> profiles = [];
  List<Json> providers = [];
  List<Json> entries = [];
  Json status = {};
  String? selected;
  String path = '/';
  String? _error;
  String? _pollError;
  String? get error => _error ?? _pollError;
  set error(String? value) {
    _error = value;
    if (value == null) _pollError = null;
  }

  String activity = '';
  bool _busy = false;
  final Set<int> _engineOperations = {};
  bool get busy => _busy || _engineOperations.isNotEmpty;
  set busy(bool value) => _busy = value;
  Future<void>? _statusRefresh;
  Future<void>? _polling;
  bool _disposed = false;
  DateTime? _directoryChecked;
  bool _directoryStale = true;
  bool available = false;
  bool quitting = false;
  int _id = 0;
  int _browseGeneration = 0;
  int? activeId;
  Timer? _poll;
  bool get connected => status['connected'] == true;
  Json get sharing => (status['sharing'] as Json?) ?? {};
  bool get hosting => sharing['running'] == true;
  bool get syncError =>
      status['backup_failed'] == true ||
      status['backup']?['state'] == 'error' ||
      (status['cache']?['deletes_rejected'] as num? ?? 0) > 0;
  bool get paused => status['paused'] == true;
  List<Json> get syncActivity => [
    ...((status['backup']?['active'] as List?) ?? []),
    ...((status['mount_activity']?['active'] as List?) ?? []),
  ].map((v) => Map<String, dynamic>.from(v as Map)).toList();
  bool get syncing =>
      busy ||
      syncActivity.isNotEmpty ||
      (status['backup']?['state'] == 'scanning' ||
          status['backup']?['state'] == 'starting') ||
      (status['cache']?['pending'] as num? ?? 0) > 0;
  Future<void> pauseToggle() => action(paused ? '恢复同步' : '暂停同步', () async {
    status = await request('pause', {'paused': !paused});
  });
  Future<void> signOut() => action('退出登录', () async {
    await request('sign_out', {'id': selected});
    entries = [];
    status = {};
    await loadProfiles();
    await refreshStatus();
  });
  bool get canMount => status['capabilities']?['mount'] == true;
  Json get nativeItemCapabilities => Map<String, dynamic>.from(
    status['local_files']?['item_status'] as Map? ?? {},
  );
  bool get canInspectNativeItems =>
      nativeItemCapabilities['schema_version'] == 1 &&
      nativeItemCapabilities['supported'] == true &&
      nativeItemCapabilities['scope'] == 'generated_fixture_only';
  Future<Json> nativeItems(String op, [String? id, bool? offline]) async {
    if (!canInspectNativeItems ||
        !const [
          'local_items',
          'local_read',
          'local_dehydrate',
          'local_pin',
          'local_unpin',
          'local_offline',
          'local_reconnect',
        ].contains(op) ||
        (const ['local_pin', 'local_unpin'].contains(op) &&
            nativeItemCapabilities['pin'] != true) ||
        (op == 'local_offline' &&
            (nativeItemCapabilities['offline_test'] != true ||
                offline == null)) ||
        (op == 'local_reconnect' &&
            nativeItemCapabilities['reconnect'] != true) ||
        (op == 'local_read' && nativeItemCapabilities['read'] != true) ||
        (op == 'local_dehydrate' &&
            nativeItemCapabilities['native_copy_dehydrate'] != true)) {
      throw StateError('本机文件能力不可用');
    }
    final result = await request(op, {
      'schema_version': 1,
      'id': ?id,
      'offline': ?offline,
    });
    if (result['schema_version'] != 1 ||
        result['scope'] != 'generated_fixture_only' ||
        result['items'] is! List) {
      throw StateError('本机文件状态版本不兼容');
    }
    return result;
  }

  bool get canPreflightLocalFile =>
      status['local_files']?['schema_version'] == 1 &&
      status['local_files']?['metadata_preflight'] == true;
  Future<Json> preflightLocalFile(String source, String kind) async {
    if (!canPreflightLocalFile) {
      throw StateError('当前引擎不支持本地文件预检');
    }
    final result = await request('local_preflight', {
      'schema_version': 1,
      'source_path': source,
      'source_kind': kind,
    });
    if (result['schema_version'] != 1 ||
        result['approval_required'] != true ||
        result['content_read'] != false ||
        result['hydration_requested'] != false ||
        result['import_performed'] != false) {
      throw StateError('本地预检响应不兼容，请更新引擎与界面');
    }
    return result;
  }

  bool get mounted => status['mounted'] == true;
  Json? get profile => profiles.where((p) => p['id'] == selected).firstOrNull;
  bool capability(String key) =>
      connected && status['capabilities']?[key] == true;
  bool get canUpload =>
      capability('upload') && capability('conditional_upload');
  bool get canMkdir => capability('mkdir');
  bool get canRename => capability('rename');
  bool get canDelete => capability('delete');
  bool get canBackup => connected
      ? capability('backup')
      : profile?['capabilities']?['backup'] == true;
  bool get writable => canUpload || canMkdir || canRename || canDelete;
  String get providerLabel =>
      profile?['provider_label'] as String? ?? 'TwoDrive';
  String get accountLabel => profile?['account_label'] as String? ?? '账号';
  String get account => connected
      ? (status['account_name'] as String? ??
            profile?['account_name'] as String? ??
            '账号名称暂不可用')
      : '未登录';
  String get trayTitle {
    final connection = connected ? ' · $providerLabel · $account' : '';
    final host = hosting ? ' · 正在共享 ${sharing['settings']?['name'] ?? ''}' : '';
    return 'TwoDrive Development$connection$host${!connected && !hosting ? ' · 未连接' : ''}';
  }

  String get trayState => error != null || syncError
      ? 'error'
      : connected && proxyFallback
      ? 'warning'
      : paused
      ? 'paused'
      : syncing
      ? 'syncing'
      : connected || hosting
      ? 'connected'
      : 'offline';
  Json get target => {
    'profile': selected,
    'revision': profile?['revision'] ?? '',
  };
  bool get proxyFallback => connected && status['route_fallback'] == true;
  void requireProfile(Json args) {
    if (!connected ||
        (args['profile'] != null && args['profile'] != selected)) {
      throw StateError('文件操作所属连接已改变，请刷新后重试');
    }
    if (args['revision'] != null && args['revision'] != target['revision']) {
      throw StateError('连接配置已更新，请刷新后重新选择文件');
    }
  }

  Future<void> start(
    String engine,
    String state,
    String peer,
    String launcher, {
    String? nativeFixtureRoot,
  }) async {
    try {
      final p = await Process.start(engine, [
        '--state',
        state,
        '--peer-bin',
        peer,
        '--launcher',
        launcher,
        if (nativeFixtureRoot != null) ...[
          '--native-local-fixture-root',
          nativeFixtureRoot,
        ],
      ]);
      _process = p;
      p.stdout.transform(utf8.decoder).transform(const LineSplitter()).listen((
        line,
      ) {
        try {
          accept(jsonDecode(line) as Json);
        } catch (_) {
          fail('引擎返回无效数据');
        }
      }, onError: (_) => fail('引擎通信中断'));
      // Keep stderr drained without exposing private configuration in the UI.
      p.stderr.drain<void>();
      unawaited(
        p.exitCode.then((code) {
          if (!quitting) {
            fail('本地引擎已退出（$code）。请重新启动客户端。');
          }
        }),
      );
      available = true;
      // Older engines keep working through single-flight polling. Operation
      // notifications are opt-in so older frontends never misread them as replies.
      try {
        await request('subscribe', {'operations': true});
      } on StateError {
        // No subscription operation in earlier engine versions.
      }
      await loadProfiles();
      await refreshStatus();
      final version = status['version'];
      if (version is String) {
        unawaited(
          updates.start(
            version,
            File('$state/update-settings.json'),
            managed: File(
              '${File(engine).parent.path}/distribution-msix',
            ).existsSync(),
          ),
        );
      }
      _poll = Timer.periodic(const Duration(seconds: 3), (_) {
        unawaited(pollNow());
      });
    } catch (e) {
      fail('无法启动本地引擎：$e');
    }
  }

  @visibleForTesting
  void accept(Json message) {
    if (_disposed) return;
    final id = message['id'] as int;
    if (id > _id) _id = id;
    if (message['event'] == 'operation') {
      if (message['running'] == true) {
        _engineOperations.add(id);
        if (!_busy) {
          activeId = id;
          activity = '正在处理连接或文件操作';
        }
      } else {
        _engineOperations.remove(id);
        if (!_busy && activeId == id) {
          activeId = null;
          activity = '';
        }
        if (message['configuration'] is Map) {
          _applyProfiles(
            Map<String, dynamic>.from(message['configuration'] as Map),
          );
        }
        if (message['status'] is Map) {
          _applyStatus(Map<String, dynamic>.from(message['status'] as Map));
        }
        _directoryStale = true;
        _browseGeneration++;
        if (!quitting && message['op'] != 'quit') unawaited(pollNow());
      }
      notifyListeners();
      return;
    }
    if (message['event'] == 'progress') {
      final task = transfers.where((t) => t.id == id).firstOrNull;
      if (task != null) {
        task.done = message['done'] as int;
        task.total = message['total'] as int;
        notifyListeners();
      }
      return;
    }
    if (message.containsKey('event')) return;
    final reply = _pending.remove(id);
    if (reply == null) {
      return;
    }
    if (message['ok'] == true) {
      reply.complete(Map<String, dynamic>.from(message['result'] as Map));
    } else {
      reply.completeError(StateError(message['error'] as String));
    }
  }

  @visibleForTesting
  void fail(String message) {
    available = false;
    _engineOperations.clear();
    status = {};
    entries = [];
    _browseGeneration++;
    error = message;
    for (final pending in _pending.values) {
      pending.completeError(StateError(message));
    }
    _pending.clear();
    notifyListeners();
  }

  Future<Json> request(String op, [Json args = const {}, int? requestId]) {
    if (!available || _process == null) {
      return Future.error(StateError('本地引擎未运行'));
    }
    if (_pending.length >= 32 && op != 'cancel') {
      return Future.error(StateError('请求正在处理中，请稍后重试'));
    }
    final id = requestId ?? ++_id;
    if (_pending.containsKey(id)) {
      return Future.error(StateError('请求编号重复'));
    }
    final completer = Completer<Json>();
    _pending[id] = completer;
    try {
      _process!.stdin.writeln(jsonEncode({'id': id, 'op': op, 'args': args}));
    } catch (e) {
      _pending.remove(id);
      completer.completeError(e);
    }
    return completer.future;
  }

  void _applyProfiles(Json result) {
    profiles = (result['profiles'] as List).cast<Json>();
    providers = ((result['providers'] as List?) ?? []).cast<Json>();
    selected = connected
        ? status['profile'] as String?
        : result['selected'] as String?;
  }

  Future<void> loadProfiles() async {
    _applyProfiles(await request('profiles'));
    notifyListeners();
  }

  void _applyStatus(Json next) {
    final previous = status['profile'];
    final wasConnected = connected;
    status = next;
    if (connected) selected = status['profile'] as String? ?? selected;
    if (!connected ||
        previous != status['profile'] ||
        wasConnected != connected) {
      entries = [];
      path = '/';
      _browseGeneration++;
      _directoryStale = true;
    }
    _pollError = status['gateway_failed'] == true
        ? '配对连接已断开，请重连。'
        : status['mount_failed'] == true
        ? '挂载进程已退出；请卸载后重试。缓存已保留。'
        : null;
  }

  // Keep the in-flight request until its reply arrives, including time spent
  // behind a long engine operation. A timer tick must never enqueue another.
  Future<void> refreshStatus() => _statusRefresh ??= _readStatus();
  Future<void> _readStatus() async {
    try {
      final next = await request('status');
      if (!_disposed) _applyStatus(next);
    } catch (e) {
      if (!_disposed) _pollError = '$e'.replaceFirst('Bad state: ', '');
      rethrow;
    } finally {
      _statusRefresh = null;
      if (!_disposed) notifyListeners();
    }
  }

  Future<void> pollNow({bool forceDirectory = false}) {
    if (forceDirectory) _directoryStale = true;
    return _polling ??= _pollOnce();
  }

  Future<void> _pollOnce() async {
    await Future<void>.value();
    try {
      if (busy || !available || quitting || _disposed) return;
      final previous = selected;
      await refreshStatus();
      if (_disposed || quitting || !available) return;
      if (selected != previous) await loadProfiles();
      if (!busy &&
          connected &&
          available &&
          !quitting &&
          (_directoryStale ||
              _directoryChecked == null ||
              DateTime.now().difference(_directoryChecked!) >=
                  const Duration(seconds: 15))) {
        await readDirectory(path);
      }
    } catch (e) {
      if (!_disposed) {
        _pollError = '$e'.replaceFirst('Bad state: ', '');
        notifyListeners();
      }
    } finally {
      _polling = null;
    }
  }

  Future<void> action(String label, Future<void> Function() run) async {
    if (busy) {
      return;
    }
    busy = true;
    activity = label;
    error = null;
    notifyListeners();
    try {
      await run();
    } catch (e) {
      error = e.toString().replaceFirst('Bad state: ', '');
    } finally {
      busy = false;
      activity = '';
      activeId = null;
      notifyListeners();
      unawaited(pollNow());
    }
  }

  Future<void> connect([String? profileId]) => action('正在登录并连接', () async {
    final id = profileId ?? selected;
    if (id == null) throw StateError('请先添加连接');
    if (connected && status['profile'] == id) return;
    _browseGeneration++;
    activeId = ++_id;
    try {
      final result = await request('connect', {'id': id}, activeId);
      selected = id;
      path = '/';
      entries = (result['entries'] as List)
          .map((e) => {...e as Json, ...target})
          .toList();
    } finally {
      await refreshStatus();
      if (connected) selected = status['profile'] as String;
      notifyListeners();
    }
  });
  Future<void> disconnect() => action('正在断开', () async {
    await request('disconnect');
    _browseGeneration++;
    entries = [];
    path = '/';
    await refreshStatus();
  });
  Future<void> browse(String target) =>
      action('读取目录', () => readDirectory(target));
  Future<void> readDirectory(String target) async {
    final generation = ++_browseGeneration;
    final identity = this.target;
    final result = await request('list', {'path': target, ...identity});
    if (generation != _browseGeneration || !available || _disposed) {
      return;
    }
    _directoryChecked = DateTime.now();
    _directoryStale = false;
    path = target;
    entries = (result['entries'] as List)
        .map((e) => {...e as Json, ...identity})
        .toList();
    notifyListeners();
  }

  Future<void> mutate(String op, Json args) => action('正在更新文件', () async {
    requireProfile(args);
    await request(op, {...target, ...args});
    await readDirectory(path);
  });
  Future<void> transfer(String op, Json args) =>
      action(op == 'upload' ? '正在上传' : '正在下载', () async {
        requireProfile(args);
        args = {...target, ...args};
        final id = ++_id;
        activeId = id;
        final task = Transfer(id, op, args);
        transfers.insert(0, task);
        if (transfers.length > 100) {
          transfers.removeLast();
        }
        notifyListeners();
        try {
          await request(op, args, id);
          task.state = '已完成';
          task.done = task.total;
          if (op == 'upload') {
            await readDirectory(path);
          }
        } catch (e) {
          task.state = '未完成';
          task.error = '$e';
          rethrow;
        }
      });
  Future<void> cancel() async {
    final id = activeId;
    if (id != null) {
      activity = '正在取消，等待当前网络读写返回';
      notifyListeners();
      await request('cancel', {'id': id});
    }
  }

  Future<void> mountToggle() => action(mounted ? '正在卸载' : '正在挂载', () async {
    activeId = ++_id;
    await request(mounted ? 'unmount' : 'mount', {}, activeId);
    await refreshStatus();
  });
  Future<String?> save(Json profile) async {
    String? id;
    await action('保存连接', () async {
      final result = await request('save_profile', profile);
      id = result['id'] as String?;
      await loadProfiles();
    });
    return id;
  }

  Future<Json?> shareAction(String op, Json args) async {
    Json? result;
    await action(
      op == 'share_start'
          ? '正在启动共享'
          : op == 'share_export'
          ? '正在导出连接文件'
          : '正在更新共享',
      () async {
        activeId = ++_id;
        result = await request(op, args, activeId);
        await refreshStatus();
      },
    );
    return result;
  }

  Future<void> remove(String id) => action('删除连接配置', () async {
    await request('remove_profile', {'id': id});
    await loadProfiles();
  });
  Future<void> autostart(bool enabled) => action('更新自启动设置', () async {
    status = await request('autostart', {'enabled': enabled});
  });
  Future<void> retryDeletes() => action('安排云端删除重试', () async {
    await request('retry_deletes', {'id': status['profile'] ?? selected});
    await refreshStatus();
  });
  Future<void> clearCache() => action('释放已同步缓存', () async {
    await request('clear_cache', {'id': selected});
    await refreshStatus();
  });
  Future<bool> shutdown() async {
    if (busy) {
      error = '请先完成或取消当前操作，再退出。';
      notifyListeners();
      return false;
    }
    try {
      if (available) {
        await request('quit');
      }
      quitting = true;
      updates.dispose();
      _poll?.cancel();
      await _process?.stdin.close();
      return true;
    } catch (e) {
      error = '$e';
      notifyListeners();
      return false;
    }
  }

  @override
  void dispose() {
    _disposed = true;
    if (!quitting) updates.dispose();
    _poll?.cancel();
    super.dispose();
  }
}
