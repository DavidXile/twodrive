import 'dart:async';
import 'dart:io';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:file_selector/file_selector.dart';
import 'status_tray.dart';
import 'tray_panel.dart';
import 'backup_page.dart';
import 'sharing_page.dart';
import 'package:window_manager/window_manager.dart';
import 'client.dart';
import 'profile_dialog.dart';
import 'local_file_preflight.dart';
import 'native_local_files.dart';
import 'file_manager.dart';

String bytes(num value) {
  if (value < 1024) {
    return '$value B';
  }
  if (value < 1048576) {
    return '${(value / 1024).toStringAsFixed(1)} KB';
  }
  if (value < 1073741824) {
    return '${(value / 1048576).toStringAsFixed(1)} MB';
  }
  return '${(value / 1073741824).toStringAsFixed(1)} GB';
}

String childPath(String parent, String name) =>
    '${parent == '/' ? '' : parent}/$name';
String parentPath(String path) {
  final index = path.lastIndexOf('/');
  return index <= 0 ? '/' : path.substring(0, index);
}

bool trayReady = false;

class Workspace extends StatefulWidget {
  const Workspace({super.key, required this.client, required this.desktop});
  final StorageClient client;
  final bool desktop;
  @override
  State<Workspace> createState() => _WorkspaceState();
}

class _WorkspaceState extends State<Workspace> with WindowListener {
  StorageClient get c => widget.client;
  final tray = StatusTray();
  bool panel = false;
  bool presenting = false;
  int page = 0;
  String filter = '';
  String _traySignature = '';
  Future<void> _trayWork = Future.value();
  @override
  void initState() {
    super.initState();
    c.addListener(changed);
    if (widget.desktop) {
      windowManager.addListener(this);
      tray.onQuit = () => unawaited(quit());
      tray.onActivate = (position) => unawaited(showPanel(position));
      unawaited(initTray());
    }
  }

  Future<void> initTray() async {
    try {
      await tray.icon('offline');
      trayReady = true;
      await windowManager.setPreventClose(true);
      await updateTray();
    } catch (_) {
      c.error = '系统托盘不可用，关闭窗口将退出客户端。';
      changed();
    }
  }

  void changed() {
    if (mounted) {
      setState(() {});
    }
    if (widget.desktop && trayReady) {
      _trayWork = _trayWork.then((_) => updateTray()).catchError((Object _) {});
    }
  }

  Future<void> updateTray() async {
    final signature =
        '${c.connected}:${c.busy}:${c.error}:${c.mounted}:${c.proxyFallback}:${c.paused}:${c.syncing}:${c.syncError}:${c.trayTitle}';
    if (_traySignature == signature) {
      return;
    }
    _traySignature = signature;
    final icon = c.trayState;
    await tray.icon(icon, title: c.trayTitle);
  }

  @override
  void onWindowFocus() => unawaited(c.pollNow(forceDirectory: true));

  @override
  void onWindowClose() {
    unawaited(windowManager.hide());
  }

  @override
  void onWindowBlur() {
    if (!Platform.isLinux && !Platform.isWindows && panel && !presenting) {
      unawaited(windowManager.hide());
    }
  }

  Future<void> showPanel(Offset position) async {
    if (presenting) return;
    if (panel && await windowManager.isVisible()) {
      await windowManager.hide();
      return;
    }
    presenting = true;
    await windowManager.hide();
    setState(() => panel = true);
    await windowManager.setFullScreen(false);
    await windowManager.unmaximize();
    for (var i = 0; i < 20 && await windowManager.isMaximized(); i++) {
      await Future<void>.delayed(const Duration(milliseconds: 50));
    }
    await windowManager.setTitleBarStyle(TitleBarStyle.hidden);
    await windowManager.setSkipTaskbar(true);
    await windowManager.setAlwaysOnTop(true);
    if (Platform.isWindows) {
      await windowManager.setMinimumSize(const Size(390, 570));
    }
    await tray.panel(true);
    if (!Platform.isLinux && !Platform.isWindows) {
      await windowManager.show();
      await windowManager.focus();
    }
    presenting = false;
  }

  Future<void> showManagement([int destination = 0]) async {
    presenting = true;
    setState(() {
      panel = false;
      page = destination;
    });
    await windowManager.setAlwaysOnTop(false);
    await windowManager.setSkipTaskbar(false);
    await tray.panel(false);
    await windowManager.setTitleBarStyle(TitleBarStyle.normal);
    await windowManager.setMinimumSize(const Size(840, 580));
    await windowManager.setSize(const Size(1120, 760));
    await windowManager.center();
    await windowManager.show();
    await windowManager.focus();
    presenting = false;
  }

  Future<void> show() => showManagement(page);
  Future<void> openMount() async {
    if (c.mounted) {
      await c.action(
        '打开本地目录',
        () => openLocalDirectory(c.profile!['mount_path'] as String),
      );
    }
  }

  Future<void> signOut() async {
    await showManagement(2);
    if (await confirm('退出登录？', '断开此账号并移除客户端中保存的登录信息。服务器文件、备份源和独立的设备配对状态会保留。')) {
      await c.signOut();
    }
  }

  Future<void> quit() async {
    if (await c.shutdown()) {
      if (trayReady) {
        await tray.destroy();
      }
      await SystemNavigator.pop();
    } else {
      await show();
    }
  }

  @override
  void dispose() {
    c.removeListener(changed);
    if (widget.desktop) {
      windowManager.removeListener(this);
      tray.onActivate = null;
    }
    super.dispose();
  }

  Future<String?> nameDialog(String title, [String initial = '']) async {
    final text = TextEditingController(text: initial);
    String? validation;
    final result = await showDialog<String>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, update) => AlertDialog(
          title: Text(title),
          content: SizedBox(
            width: 380,
            child: TextField(
              controller: text,
              autofocus: true,
              decoration: InputDecoration(
                labelText: '名称',
                errorText: validation,
              ),
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: const Text('取消'),
            ),
            FilledButton(
              onPressed: () {
                final value = text.text.trim();
                if (value.isEmpty ||
                    value == '.' ||
                    value == '..' ||
                    value.contains(RegExp(r'[/\\\x00]'))) {
                  update(() => validation = '请输入有效的单个文件名');
                  return;
                }
                Navigator.pop(context, value);
              },
              child: const Text('确定'),
            ),
          ],
        ),
      ),
    );
    // The dialog route may animate out while its text field is still mounted.
    Future<void>.delayed(const Duration(seconds: 1), text.dispose);
    return result;
  }

  Future<bool> confirm(String title, String message) async =>
      await showDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: Text(title),
          content: Text(message),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('取消'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('确认'),
            ),
          ],
        ),
      ) ??
      false;
  Future<void> upload() async {
    final identity = c.target;
    final directory = c.path;
    final files = await openFiles(confirmButtonText: '上传');
    for (final file in files) {
      if (!c.connected || c.busy) {
        break;
      }
      await c.transfer('upload', {
        ...identity,
        'source': file.path,
        'path': childPath(directory, file.name),
      });
      if (c.error != null) {
        break;
      }
    }
  }

  Future<void> download(Json file) async {
    final target = await getSaveLocation(
      suggestedName: file['name'] as String,
      confirmButtonText: '下载',
    );
    if (target != null) {
      await c.transfer('download', {
        'profile': file['profile'],
        'revision': file['revision'],
        'path': file['path'],
        'output': target.path,
        'size': file['size'],
      });
    }
  }

  Future<void> fileAction(String action, Json file) async {
    switch (action) {
      case 'download':
        await download(file);
      case 'rename':
        final name = await nameDialog('重命名', file['name'] as String);
        if (name != null) {
          await c.mutate('rename', {
            'profile': file['profile'],
            'revision': file['revision'],
            'path': file['path'],
            'to': childPath(parentPath(file['path'] as String), name),
          });
        }
      case 'delete':
        if (await confirm(
          '删除 ${file['name']}？',
          file['directory'] == true ? '文件夹及其内容将从服务器删除。' : '此文件将从服务器删除。',
        )) {
          await c.mutate('delete', {
            'path': file['path'],
            'profile': file['profile'],
            'revision': file['revision'],
          });
        }
      case 'replace':
        final source = await openFile(confirmButtonText: '选择新内容');
        if (source != null &&
            await confirm('替换 ${file['name']}？', '仅在服务器版本未发生变化时覆盖。')) {
          await c.transfer('upload', {
            'profile': file['profile'],
            'revision': file['revision'],
            'path': file['path'],
            'source': source.path,
            'etag': file['etag'],
          });
        }
    }
  }

  @override
  Widget build(BuildContext context) {
    if (panel) {
      return TrayPanel(
        client: c,
        onManage: showManagement,
        onOpenFolder: () async {
          if (!c.canMount) {
            await showManagement(0);
            return;
          }
          if (!c.mounted) {
            await c.mountToggle();
          }
          await openMount();
        },
        onQuit: quit,
        onSignOut: signOut,
      );
    }
    final titles = ['文件', '传输', '连接', '备份', '共享', '设置'];
    return Scaffold(
      body: Row(
        children: [
          Container(
            width: 190,
            color: Colors.white,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Expanded(
                  child: ListView(
                    key: const ValueKey('navigation'),
                    children: [
                      const Padding(
                        padding: EdgeInsets.fromLTRB(22, 16, 20, 8),
                        child: Icon(
                          Icons.cloud_outlined,
                          size: 32,
                          color: Color(0xff326bd6),
                        ),
                      ),
                      const Text(
                        'TwoDrive',
                        textAlign: TextAlign.center,
                        style: TextStyle(
                          fontSize: 23,
                          fontWeight: FontWeight.w700,
                        ),
                      ),
                      const Padding(
                        padding: EdgeInsets.only(bottom: 12, top: 3),
                        child: Text(
                          '文件与同步',
                          textAlign: TextAlign.center,
                          style: TextStyle(color: Colors.blueGrey),
                        ),
                      ),
                      for (var i = 0; i < titles.length; i++)
                        Padding(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 12,
                            vertical: 2,
                          ),
                          child: ListTile(
                            dense: true,
                            selected: page == i,
                            selectedTileColor: const Color(0xffedf3ff),
                            shape: RoundedRectangleBorder(
                              borderRadius: BorderRadius.circular(12),
                            ),
                            leading: Icon(
                              [
                                Icons.folder_outlined,
                                Icons.swap_vert,
                                Icons.dns_outlined,
                                Icons.backup_outlined,
                                Icons.share_outlined,
                                Icons.tune,
                              ][i],
                            ),
                            title: Text(titles[i]),
                            onTap: () => setState(() => page = i),
                          ),
                        ),
                    ],
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.all(20),
                  child: Text(
                    c.connected ? '● 已连接' : '○ 未连接',
                    style: TextStyle(
                      color: c.connected ? Colors.teal : Colors.blueGrey,
                    ),
                  ),
                ),
                if (widget.desktop)
                  TextButton.icon(
                    onPressed: quit,
                    icon: const Icon(Icons.power_settings_new, size: 18),
                    label: const Text('退出客户端'),
                  ),
                const SizedBox(height: 18),
              ],
            ),
          ),
          Expanded(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(28, 26, 28, 16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Row(
                    children: [
                      Text(
                        titles[page],
                        style: const TextStyle(
                          fontSize: 28,
                          fontWeight: FontWeight.w700,
                        ),
                      ),
                      const Spacer(),
                      if (c.connected)
                        Chip(
                          label: Text(c.writable ? '读写' : '只读'),
                          avatar: Icon(
                            c.writable
                                ? Icons.edit_outlined
                                : Icons.lock_outline,
                            size: 16,
                          ),
                        ),
                      const SizedBox(width: 12),
                      FilledButton.tonalIcon(
                        onPressed: c.busy || c.selected == null || !c.available
                            ? null
                            : c.connected
                            ? c.disconnect
                            : c.connect,
                        icon: Icon(c.connected ? Icons.link_off : Icons.link),
                        label: Text(c.connected ? '断开' : '连接'),
                      ),
                    ],
                  ),
                  Padding(
                    padding: const EdgeInsets.only(top: 6, bottom: 20),
                    child: Text(
                      c.connected
                          ? '${c.profile?['name'] ?? c.providerLabel} · ${c.account}'
                          : c.profile?['name'] as String? ?? '添加存储账号或已配对设备',
                      style: const TextStyle(color: Colors.blueGrey),
                    ),
                  ),
                  if (c.proxyFallback)
                    const Card(
                      color: Color(0xfffff2cd),
                      child: Padding(
                        padding: EdgeInsets.all(12),
                        child: Row(
                          children: [
                            Icon(
                              Icons.warning_amber_rounded,
                              color: Color(0xffe0a000),
                            ),
                            SizedBox(width: 8),
                            Expanded(
                              child: Text('直连未能完成，已使用系统／代理兜底。可在设置中查看网络路径。'),
                            ),
                          ],
                        ),
                      ),
                    ),
                  if (c.error != null)
                    Container(
                      margin: const EdgeInsets.only(bottom: 12),
                      padding: const EdgeInsets.all(12),
                      decoration: BoxDecoration(
                        color: const Color(0xffffeee9),
                        borderRadius: BorderRadius.circular(10),
                      ),
                      child: Row(
                        children: [
                          const Icon(
                            Icons.error_outline,
                            color: Colors.deepOrange,
                          ),
                          const SizedBox(width: 10),
                          Expanded(
                            child: ConstrainedBox(
                              constraints: const BoxConstraints(maxHeight: 120),
                              child: SingleChildScrollView(
                                child: SelectableText(c.error!),
                              ),
                            ),
                          ),
                          IconButton(
                            tooltip: '关闭提示',
                            onPressed: () {
                              c.error = null;
                              changed();
                            },
                            icon: const Icon(Icons.close),
                          ),
                        ],
                      ),
                    ),
                  if (c.busy)
                    Column(
                      children: [
                        const LinearProgressIndicator(),
                        Row(
                          children: [
                            Expanded(
                              child: Padding(
                                padding: const EdgeInsets.symmetric(
                                  vertical: 8,
                                ),
                                child: Text(c.activity),
                              ),
                            ),
                            if (c.activeId != null)
                              TextButton(
                                onPressed: c.cancel,
                                child: const Text('取消'),
                              ),
                          ],
                        ),
                      ],
                    ),
                  Expanded(
                    child: switch (page) {
                      0 => filesPage(),
                      1 => transfersPage(),
                      2 => connectionsPage(),
                      3 => BackupPage(key: ValueKey(c.selected), client: c),
                      4 => SharingPage(client: c),
                      _ => settingsPage(),
                    },
                  ),
                  if (c.updates.available)
                    ListTile(
                      dense: true,
                      leading: const Icon(Icons.system_update_alt),
                      title: Text(
                        'Development ${c.updates.latest!.version} 可用',
                      ),
                      trailing: TextButton(
                        onPressed: c.updates.openRelease,
                        child: const Text('查看更新'),
                      ),
                    ),
                  const SizedBox(height: 10),
                  Text(
                    c.mounted
                        ? '已挂载 · ${c.profile?['mount_path']}'
                        : '文件浏览 · 关闭窗口后可从云朵托盘打开',
                    style: const TextStyle(
                      fontSize: 12,
                      color: Colors.blueGrey,
                    ),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget empty(IconData icon, String title, String detail, [Widget? action]) =>
      Center(
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(icon, size: 60, color: Colors.blueGrey.shade300),
              const SizedBox(height: 16),
              Text(
                title,
                style: const TextStyle(
                  fontSize: 20,
                  fontWeight: FontWeight.w600,
                ),
              ),
              const SizedBox(height: 8),
              Text(
                detail,
                textAlign: TextAlign.center,
                style: const TextStyle(color: Colors.blueGrey),
              ),
              if (action != null)
                Padding(padding: const EdgeInsets.only(top: 18), child: action),
            ],
          ),
        ),
      );
  Widget filesPage() {
    if (!c.connected) {
      return empty(
        Icons.cloud_outlined,
        '连接你的文件',
        '选择存储提供者并连接你的账号或设备。',
        OutlinedButton(
          onPressed: () => setState(() => page = 2),
          child: const Text('管理连接'),
        ),
      );
    }
    final entries = c.entries
        .where(
          (e) => (e['name'] as String).toLowerCase().contains(
            filter.toLowerCase(),
          ),
        )
        .toList();
    return Column(
      children: [
        Row(
          children: [
            IconButton(
              tooltip: '上一级',
              onPressed: c.busy || c.path == '/'
                  ? null
                  : () => c.browse(parentPath(c.path)),
              icon: const Icon(Icons.arrow_upward),
            ),
            Expanded(child: SelectableText(c.path, maxLines: 2)),
            IconButton(
              tooltip: '刷新',
              onPressed: c.busy ? null : () => c.browse(c.path),
              icon: const Icon(Icons.refresh),
            ),
            if (c.canMkdir || c.canUpload) ...[
              if (c.canMkdir)
                IconButton(
                  tooltip: '新建文件夹',
                  onPressed: c.busy
                      ? null
                      : () async {
                          final identity = c.target;
                          final directory = c.path;
                          final name = await nameDialog('新建文件夹');
                          if (name != null) {
                            await c.mutate('mkdir', {
                              'path': childPath(directory, name),
                              ...identity,
                            });
                          }
                        },
                  icon: const Icon(Icons.create_new_folder_outlined),
                ),
              if (c.canUpload)
                FilledButton.icon(
                  onPressed: c.busy ? null : upload,
                  icon: const Icon(Icons.upload_outlined),
                  label: const Text('上传'),
                ),
            ],
          ],
        ),
        const SizedBox(height: 12),
        TextField(
          decoration: const InputDecoration(
            prefixIcon: Icon(Icons.search),
            hintText: '筛选当前目录',
          ),
          onChanged: (value) => setState(() => filter = value),
        ),
        const SizedBox(height: 12),
        Expanded(
          child: entries.isEmpty
              ? empty(
                  Icons.folder_open,
                  '当前目录为空',
                  filter.isEmpty ? '上传文件或新建文件夹即可开始。' : '没有匹配的文件。',
                )
              : Card(
                  margin: EdgeInsets.zero,
                  color: Colors.white,
                  clipBehavior: Clip.antiAlias,
                  child: ListView.separated(
                    itemCount: entries.length,
                    separatorBuilder: (_, _) => const Divider(height: 1),
                    itemBuilder: (context, index) {
                      final file = entries[index];
                      final directory = file['directory'] == true;
                      return ListTile(
                        leading: Icon(
                          directory ? Icons.folder : Icons.description_outlined,
                          color: directory
                              ? const Color(0xffe7b448)
                              : Colors.blueGrey,
                        ),
                        title: Text(file['name'] as String),
                        subtitle: Text(
                          directory ? '文件夹' : bytes(file['size'] as int),
                        ),
                        onTap: c.busy
                            ? null
                            : directory
                            ? () => c.browse(file['path'] as String)
                            : () => download(file),
                        trailing: PopupMenuButton<String>(
                          enabled: !c.busy,
                          onSelected: (action) => fileAction(action, file),
                          itemBuilder: (_) => [
                            if (!directory)
                              const PopupMenuItem(
                                value: 'download',
                                child: Text('下载'),
                              ),
                            if (c.canRename)
                              const PopupMenuItem(
                                value: 'rename',
                                child: Text('重命名'),
                              ),
                            if (c.canUpload &&
                                !directory &&
                                (file['etag'] as String).isNotEmpty)
                              const PopupMenuItem(
                                value: 'replace',
                                child: Text('替换内容'),
                              ),
                            if (c.canDelete)
                              const PopupMenuItem(
                                value: 'delete',
                                child: Text('删除'),
                              ),
                          ],
                        ),
                      );
                    },
                  ),
                ),
        ),
        const SizedBox(height: 8),
        Align(
          alignment: Alignment.centerLeft,
          child: Text(
            '${entries.length} 项',
            style: const TextStyle(color: Colors.blueGrey),
          ),
        ),
      ],
    );
  }

  Widget transfersPage() => c.transfers.isEmpty
      ? empty(Icons.swap_vert, '暂无传输', '上传和下载的进度、结果与重试会显示在这里。')
      : ListView.builder(
          itemCount: c.transfers.length,
          itemBuilder: (context, index) {
            final task = c.transfers[index];
            return Card(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Icon(
                          task.operation == 'upload'
                              ? Icons.upload
                              : Icons.download,
                        ),
                        const SizedBox(width: 12),
                        Expanded(child: Text(task.name)),
                        Text(task.state),
                        if (task.state == '未完成')
                          TextButton(
                            onPressed:
                                c.busy ||
                                    !c.connected ||
                                    task.args['profile'] != c.selected
                                ? null
                                : () => c.transfer(task.operation, task.args),
                            child: const Text('重试'),
                          ),
                      ],
                    ),
                    const SizedBox(height: 10),
                    LinearProgressIndicator(
                      value: task.state == '已完成'
                          ? 1
                          : task.total == 0
                          ? null
                          : task.done / task.total,
                    ),
                    const SizedBox(height: 8),
                    Text('${bytes(task.done)} / ${bytes(task.total)}'),
                    if (task.error != null)
                      SelectableText(
                        task.error!,
                        style: const TextStyle(color: Colors.deepOrange),
                      ),
                  ],
                ),
              ),
            );
          },
        );
  Widget connectionsPage() => ListView(
    children: [
      if (c.connected)
        Padding(
          padding: const EdgeInsets.only(bottom: 16),
          child: Align(
            alignment: Alignment.centerLeft,
            child: OutlinedButton.icon(
              onPressed: c.busy
                  ? null
                  : () async {
                      if (await confirm(
                        '退出登录？',
                        '断开此账号并移除客户端中保存的登录信息。服务器文件、备份源和独立的设备配对状态会保留。',
                      )) {
                        await c.signOut();
                      }
                    },
              icon: const Icon(Icons.logout),
              label: const Text('退出登录'),
            ),
          ),
        ),
      Align(
        alignment: Alignment.centerLeft,
        child: FilledButton.icon(
          onPressed: c.busy || !c.available ? null : () => editProfile(),
          icon: const Icon(Icons.add),
          label: const Text('添加连接'),
        ),
      ),
      const SizedBox(height: 16),
      for (final p in c.profiles)
        Card(
          color: Colors.white,
          child: ListTile(
            leading: Icon(
              p['icon'] == 'devices' ? Icons.devices : Icons.cloud_outlined,
            ),
            title: Text(p['name'] as String),
            subtitle: Text(
              [
                p['provider_label'] as String? ?? p['kind'] as String,
                if (c.connected && c.status['profile'] == p['id'])
                  '${c.accountLabel}：${c.account}'
                else if (p['account_name'] != null)
                  '上次登录：${p['account_name']}',
                if (p['summary'] != p['provider_label'] &&
                    p['summary'] != p['name'] &&
                    p['summary'] != null)
                  p['summary'] as String,
              ].join(' · '),
            ),
            selected: c.connected && c.status['profile'] == p['id'],
            onTap: c.busy || !c.available
                ? null
                : () => unawaited(c.connect(p['id'] as String)),
            trailing: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (c.connected && c.status['profile'] == p['id'])
                  const Chip(
                    avatar: Icon(
                      Icons.check_circle,
                      size: 18,
                      color: Colors.blue,
                    ),
                    label: Text('当前使用'),
                  )
                else
                  OutlinedButton.icon(
                    onPressed: c.busy || !c.available
                        ? null
                        : () => c.connect(p['id'] as String),
                    icon: Icon(
                      c.connected ? Icons.swap_horiz : Icons.login,
                      size: 18,
                    ),
                    label: Text(c.connected ? '切换使用' : '登录并使用'),
                  ),
                IconButton(
                  tooltip: '编辑',
                  onPressed:
                      c.busy ||
                          !c.available ||
                          (c.connected && c.status['profile'] == p['id'])
                      ? null
                      : () => editProfile(p),
                  icon: const Icon(Icons.edit_outlined),
                ),
                IconButton(
                  tooltip: '删除配置',
                  onPressed:
                      c.busy ||
                          !c.available ||
                          (c.connected && c.status['profile'] == p['id'])
                      ? null
                      : () async {
                          if (await confirm('删除连接配置？', '服务器文件和已有配对状态会保留。')) {
                            await c.remove(p['id'] as String);
                          }
                        },
                  icon: const Icon(Icons.delete_outline),
                ),
              ],
            ),
          ),
        ),
      if (c.profiles.isEmpty)
        const Padding(
          padding: EdgeInsets.all(24),
          child: Text('选择存储提供者并填写连接信息。'),
        ),
    ],
  );
  Widget settingsPage() {
    final cache = c.status['cache'] as Json?;
    final network = (c.status['network'] as List?)?.cast<String>() ?? [];
    return ListView(
      children: [
        Card(
          color: Colors.white,
          child: Padding(
            padding: const EdgeInsets.all(20),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  '软件更新 · Development ${c.updates.current}',
                  style: const TextStyle(
                    fontSize: 18,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                if (c.updates.managed)
                  const Text(
                    '此 MSIX 安装包通过其分发渠道更新。商店安装请在 Microsoft Store 中检查更新。',
                  )
                else ...[
                  SwitchListTile(
                    contentPadding: EdgeInsets.zero,
                    title: const Text('自动检查更新'),
                    subtitle: const Text(
                      '每 24 小时向 GitHub 检查 Development 新版本，不自动下载或安装。',
                    ),
                    value: c.updates.enabled,
                    onChanged: c.updates.checking ? null : c.updates.setEnabled,
                  ),
                  if (c.updates.error != null) Text(c.updates.error!),
                  if (c.updates.lastSuccess != null && c.updates.error == null)
                    Text(
                      c.updates.available
                          ? '发现新版本 ${c.updates.latest!.version}'
                          : '本次检查未发现更新版本',
                    ),
                  if (c.updates.latest != null)
                    SelectableText(c.updates.latest!.url),
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 12,
                    children: [
                      OutlinedButton(
                        onPressed:
                            c.updates.checking || c.updates.current.isEmpty
                            ? null
                            : () => c.updates.check(manual: true),
                        child: Text(c.updates.checking ? '检查中…' : '立即检查'),
                      ),
                      if (c.updates.available)
                        FilledButton.tonal(
                          onPressed: c.updates.openRelease,
                          child: const Text('打开发布页面'),
                        ),
                    ],
                  ),
                ],
              ],
            ),
          ),
        ),
        Card(
          color: Colors.white,
          child: SwitchListTile(
            title: const Text('登录后自动启动'),
            subtitle: Text(
              c.updates.managed
                  ? 'MSIX 自启动尚待适配；请从开始菜单启动。'
                  : '默认关闭。开启后在托盘中运行并连接上次使用的服务器。',
            ),
            value: c.status['autostart'] == true,
            onChanged: c.busy || !c.available || c.updates.managed
                ? null
                : c.autostart,
          ),
        ),
        if (c.canPreflightLocalFile) LocalFilePreflightCard(client: c),
        if (c.canInspectNativeItems) NativeLocalFilesCard(client: c),
        if (c.nativeItemCapabilities['owner_unavailable'] == true)
          const Card(
            child: Padding(
              padding: EdgeInsets.all(20),
              child: Text('本机文件所有者不可用。原生操作已关闭，请保留目录与恢复日志。'),
            ),
          ),
        if (c.canMount)
          Card(
            color: Colors.white,
            child: Padding(
              padding: const EdgeInsets.all(20),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const Text(
                    '文件管理器挂载',
                    style: TextStyle(fontSize: 18, fontWeight: FontWeight.w600),
                  ),
                  const SizedBox(height: 10),
                  SelectableText(
                    c.profile?['mount_path'] as String? ?? '请先配置连接的挂载目录',
                  ),
                  const SizedBox(height: 12),
                  Wrap(
                    spacing: 12,
                    children: [
                      FilledButton.tonalIcon(
                        onPressed: c.busy || !c.connected
                            ? null
                            : c.mountToggle,
                        icon: Icon(c.mounted ? Icons.eject : Icons.folder_open),
                        label: Text(c.mounted ? '卸载' : '挂载'),
                      ),
                      OutlinedButton(
                        onPressed: c.mounted ? openMount : null,
                        child: const Text('在文件管理器中打开'),
                      ),
                    ],
                  ),
                  const SizedBox(height: 12),
                  const Text('挂载按需下载。写入先保存到本地缓存，待上传完成后才代表服务器已收到。'),
                ],
              ),
            ),
          ),
        if (c.canMount)
          Card(
            color: Colors.white,
            child: ListTile(
              title: Text('挂载缓存 · ${bytes(cache?['bytes'] as num? ?? 0)}'),
              subtitle: Text('待处理或冲突：${cache?['pending'] ?? 0} 项。清理保留尚未上传的数据。'),
              trailing: OutlinedButton(
                onPressed: c.busy || c.selected == null ? null : c.clearCache,
                child: const Text('释放已同步缓存'),
              ),
            ),
          ),
        if (c.canMount && (cache?['deletes_pending'] as num? ?? 0) > 0)
          Card(
            color: Colors.white,
            child: ListTile(
              title: Text('云端删除待确认：${cache?['deletes_pending']} 项'),
              subtitle: Text(
                '本地已删除。被拒绝：${cache?['deletes_rejected'] ?? 0} 项，等待重试：${cache?['deletes_deferred'] ?? 0} 项。任务已保留；修复权限或登录后可重新尝试。',
              ),
              trailing: OutlinedButton(
                onPressed: c.busy || c.selected == null ? null : c.retryDeletes,
                child: const Text('重新尝试'),
              ),
            ),
          ),
        Card(
          color: Colors.white,
          child: Padding(
            padding: const EdgeInsets.all(20),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text(
                  '网络路径',
                  style: TextStyle(fontSize: 18, fontWeight: FontWeight.w600),
                ),
                const SizedBox(height: 12),
                Text(switch (c.profile?['network_route']) {
                  'direct' => '仅直连：绑定物理网卡并绕过代理；失败时停止连接。',
                  'system' => '使用系统路由及系统配置的代理。',
                  _ => '优先直连，失败时明确提示并允许系统／代理兜底。',
                }),
                const SizedBox(height: 12),
                for (final line in network)
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 3),
                    child: SelectableText(
                      line,
                      style: const TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 12,
                      ),
                    ),
                  ),
                if (network.isEmpty)
                  const Text(
                    '当前没有网络路径报告。',
                    style: TextStyle(color: Colors.blueGrey),
                  ),
              ],
            ),
          ),
        ),
      ],
    );
  }

  Future<void> editProfile([Json? original]) async {
    final profile = await showDialog<Json>(
      context: context,
      builder: (_) =>
          ProfileDialog(original: original, providers: c.providers, client: c),
    );
    if (profile != null) {
      final connect = profile.remove('_connect') == true;
      final id = await c.save(profile);
      if (connect && id != null) await c.connect(id);
    }
  }
}
