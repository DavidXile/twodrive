import 'package:flutter/material.dart';
import 'client.dart';

/// Presentation only. Classification and filesystem access belong to the engine.
class LocalFilePreflightCard extends StatefulWidget {
  const LocalFilePreflightCard({super.key, required this.client});
  final StorageClient client;
  @override
  State<LocalFilePreflightCard> createState() => _LocalFilePreflightCardState();
}

class _LocalFilePreflightCardState extends State<LocalFilePreflightCard> {
  final path = TextEditingController();
  String sourceKind = 'unknown';
  bool checking = false;
  String? result;
  int generation = 0;
  void invalidate() => setState(() {
    generation++;
    result = null;
    checking = false;
  });
  @override
  void dispose() {
    path.dispose();
    super.dispose();
  }

  Future<void> inspect() async {
    final requestGeneration = ++generation;
    final sourcePath = path.text;
    final kind = sourceKind;
    final client = widget.client;
    bool current() =>
        mounted &&
        requestGeneration == generation &&
        path.text == sourcePath &&
        sourceKind == kind &&
        identical(widget.client, client);
    setState(() {
      checking = true;
      result = null;
    });
    try {
      final report = await client.preflightLocalFile(sourcePath, kind);
      const messages = {
        'full_file_needs_approved_plan':
            '文件属性符合完整文件候选。尚未确认云端上传完成，迁移前仍需确认范围、冲突保护、回滚和空间方案。',
        'placeholder_needs_original_client_export':
            '存在占位或特殊文件属性。需要通过原客户端导出完整副本，不能直接接管。',
        'unsupported_cache': '私有缓存无法视为完整可迁移文件，请通过原客户端导出。',
        'unsupported_virtual_mount': '虚拟挂载不支持直接接管，请通过原客户端导出。',
        'unknown_provenance': '请先确认文件来源。来源不明时不能判断能否迁移。',
        'skip_directory': '请选择一个文件；目录迁移需要另外制定范围与空间方案。',
        'skip_reparse_or_offline': '存在链接、离线或特殊文件属性，无法确认完整副本。',
      };
      if (current()) {
        setState(
          () => result = messages[report['decision']] ?? '引擎返回未知结果，暂不迁移。',
        );
      }
    } catch (_) {
      if (current()) setState(() => result = '无法完成预检。请检查绝对路径、访问权限和引擎版本。');
    } finally {
      if (current()) setState(() => checking = false);
    }
  }

  @override
  Widget build(BuildContext context) => Card(
    color: Colors.white,
    child: Padding(
      padding: const EdgeInsets.all(20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text(
            '本地文件迁移预检',
            style: TextStyle(fontSize: 18, fontWeight: FontWeight.w600),
          ),
          const SizedBox(height: 8),
          const Text('检查一个文件的属性，不读取内容或移动文件。原生文件管理器同步与目录接管尚未开放。'),
          const SizedBox(height: 12),
          TextField(
            controller: path,
            onChanged: (_) => invalidate(),
            decoration: const InputDecoration(labelText: '文件绝对路径'),
          ),
          const SizedBox(height: 12),
          DropdownButtonFormField<String>(
            initialValue: sourceKind,
            decoration: const InputDecoration(labelText: '文件来源'),
            items: const [
              DropdownMenuItem(value: 'unknown', child: Text('尚不确定')),
              DropdownMenuItem(
                value: 'ordinary_directory',
                child: Text('普通本地目录'),
              ),
              DropdownMenuItem(
                value: 'other_client_sync_root',
                child: Text('现有云盘同步目录'),
              ),
              DropdownMenuItem(value: 'virtual_mount', child: Text('虚拟挂载')),
              DropdownMenuItem(value: 'private_cache', child: Text('客户端私有缓存')),
            ],
            onChanged: (value) => setState(() {
              sourceKind = value ?? 'unknown';
              generation++;
              result = null;
              checking = false;
            }),
          ),
          const SizedBox(height: 12),
          OutlinedButton(
            onPressed: checking || widget.client.busy ? null : inspect,
            child: Text(checking ? '检查中…' : '检查文件属性'),
          ),
          if (result != null)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: Text(result!),
            ),
        ],
      ),
    ),
  );
}
