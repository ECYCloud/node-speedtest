#ifndef PROCESSES_H_INCLUDED
#define PROCESSES_H_INCLUDED

#include <string>

bool runProgram(std::string command, std::string runpath, bool wait);
void killByHandle();
bool killProgram(std::string program);
// 静默执行命令并捕获其 stdout(+stderr)文本返回。
// Windows 下用 CreateProcess + CREATE_NO_WINDOW + 匿名管道，绝不弹出 cmd 窗口;
// 这是为了替代 _popen —— _popen 在无控制台的 detached 进程里会闪一帧 cmd.exe。
std::string runCommandCapture(const std::string &command, const std::string &runpath = "");
#endif // PROCESSES_H_INCLUDED
