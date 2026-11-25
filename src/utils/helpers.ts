// 辅助函数和工具类

/**
 * 获取监控状态对应的图标
 */
export function getStatusIcon(status: string): string {
  const iconMap: Record<string, string> = {
    '空闲': '▶️',
    '准备中': '⏳',
    '警戒中': '🛡️',
    '锁定中': '🛡️' // 锁定中也显示盾牌
  };
  return iconMap[status] || '❓';
}

/**
 * 获取监控状态对应的文本
 */
export function getStatusText(status: string, shortcut: string): string {
  const textMap: Record<string, string> = {
    '空闲': '启动监控',
    '准备中': `准备中 (${shortcut} 取消)`,
    '警戒中': `警戒中 (${shortcut} 停止)`,
    '锁定中': `等待关闭 (${shortcut} 停止)`
  };
  return textMap[status] || `等待关闭 (${shortcut} 停止)`;
}

/**
 * 获取日志级别对应的CSS类名
 */
export function getLogLevelClass(level: string): string {
  switch (level.toLowerCase()) {
    case 'error': return 'log-error';
    case 'warn': return 'log-warn';
    case 'info': return 'log-info';
    case 'debug': return 'log-debug';
    default: return 'log-default';
  }
}

/**
 * 获取相机权限状态对应的CSS类名
 */
export function getPermissionStatusClass(status: string): string {
  switch (status) {
    case '已授权':
      return 'permission-granted';
    case '被拒绝':
      return 'permission-denied';
    case '未检查':
    default:
      return 'permission-unknown';
  }
}

/**
 * 验证快捷键格式是否有效
 */
export function validateShortcut(shortcut: string): boolean {
  if (!shortcut || shortcut === "按下快捷键...") return false;
  
  const parts = shortcut.split('+');
  if (parts.length < 2) return false;
  
  const modifiers = parts.slice(0, -1);
  const mainKey = parts[parts.length - 1];
  
  // 检查修饰键是否有效
  const validModifiers = ['Ctrl', 'Alt', 'Shift', 'Meta', 'Cmd'];
  for (const modifier of modifiers) {
    if (!validModifiers.includes(modifier)) return false;
  }
  
  // 检查主键是否有效（不能是修饰键）
  if (validModifiers.includes(mainKey)) return false;
  if (!mainKey || mainKey.trim() === '') return false;
  
  return true;
}
