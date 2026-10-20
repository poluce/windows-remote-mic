/**
 * 语音输出设备名（唯一事实来源）。
 *
 * 语音桥把遥控器音频写入该播放设备；诊断页的测试音也用它。
 * 之前这个字符串散落在多个页面的 invoke 参数里，改动时容易漏改。
 */
export const OUTPUT_DEVICE = "CABLE Input";
