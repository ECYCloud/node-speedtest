#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <vector>
#include <algorithm>
#include <iostream>
#include <queue>
#include <signal.h>

#include "misc.h"

#ifdef _WIN32
#include <windows.h>
#include <tlhelp32.h>
#else
#include <sys/wait.h>
#endif // _WIN32

#include "processes.h"

#ifndef _WIN32
typedef pid_t HANDLE;
#endif // _WIN32

std::queue<HANDLE> handles;
#ifdef _WIN32
HANDLE job = 0;
#else
FILE *pPipe;
#endif // _WIN32

// 静默执行命令并捕获 stdout(已合并 stderr)文本。Windows 走 CreateProcess +
// CREATE_NO_WINDOW + 匿名管道，不弹 cmd 窗口;非 Windows 退回 popen。
std::string runCommandCapture(const std::string &command, const std::string &runpath)
{
    std::string output;
#ifdef _WIN32
    SECURITY_ATTRIBUTES sa = {};
    sa.nLength = sizeof(sa);
    sa.bInheritHandle = TRUE;        // 管道句柄需可被子进程继承
    sa.lpSecurityDescriptor = NULL;

    HANDLE rd = NULL, wr = NULL;
    if(!CreatePipe(&rd, &wr, &sa, 0))
        return output;
    // 读取端不被子进程继承，避免子进程持有导致 ReadFile 永不返回 EOF
    SetHandleInformation(rd, HANDLE_FLAG_INHERIT, 0);

    STARTUPINFO si = {};
    si.cb = sizeof(si);
    si.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
    si.wShowWindow = SW_HIDE;
    si.hStdOutput = wr;
    si.hStdError  = wr;
    si.hStdInput  = GetStdHandle(STD_INPUT_HANDLE);

    PROCESS_INFORMATION pi = {};
    std::string cmdline = command;          // CreateProcess 会就地修改，需可写缓冲
    std::vector<char> buf(cmdline.begin(), cmdline.end());
    buf.push_back('\0');
    const char *workdir = runpath.empty() ? NULL : runpath.c_str();

    BOOL ok = CreateProcess(NULL, buf.data(), NULL, NULL, TRUE,
                            CREATE_NO_WINDOW, NULL, workdir, &si, &pi);
    CloseHandle(wr);                        // 父进程关闭写端，否则读不到 EOF
    if(!ok)
    {
        CloseHandle(rd);
        return output;
    }
    char rbuf[4096];
    DWORD n = 0;
    while(ReadFile(rd, rbuf, sizeof(rbuf), &n, NULL) && n > 0)
        output.append(rbuf, n);
    CloseHandle(rd);
    WaitForSingleObject(pi.hProcess, 5000);
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);
#else
    std::string full = runpath.empty() ? command
                                       : ("cd \"" + runpath + "\" && " + command);
    FILE *pipe = popen(full.c_str(), "r");
    if(pipe)
    {
        char rbuf[4096];
        size_t n;
        while((n = fread(rbuf, 1, sizeof(rbuf), pipe)) > 0)
            output.append(rbuf, n);
        pclose(pipe);
    }
#endif
    return output;
}

bool runProgram(std::string command, std::string runpath, bool wait)
{
#ifdef _WIN32
    BOOL retval = false;
    STARTUPINFO si = {};
    si.cb = sizeof(STARTUPINFO);
    PROCESS_INFORMATION pi = {};
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION job_limits = {};
    char curdir[512] = {}, *cmdstr = {}, *pathstr = {};
    std::string path;
    job = CreateJobObject(NULL, NULL);
    job_limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

    if(strFind(runpath, ":")) //is an absolute path
    {
        path = runpath;
    }
    else //is a relative path
    {
        GetCurrentDirectory(512, curdir);
        path = std::string(curdir) + "\\";
        if(runpath.size())
            path += runpath + "\\";
    }
    cmdstr = const_cast<char*>(command.data());
    pathstr = const_cast<char*>(path.data());
    retval = CreateProcess(NULL, cmdstr, NULL, NULL, false, CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB, NULL, pathstr, &si, &pi);
    if(retval == FALSE)
        return false;

    sleep(600); //slow down to prevent some problem
    DWORD ExitCode = STILL_ACTIVE;

    do
    {
        retval = GetExitCodeProcess(pi.hProcess, &ExitCode);
        if(retval == FALSE)
            continue;
        else if(ExitCode == STILL_ACTIVE)
            break;
        else
            return ExitCode;
    }
    while(true);

    AssignProcessToJobObject(job, pi.hProcess);
    SetInformationJobObject(job, JobObjectExtendedLimitInformation, &job_limits, sizeof(job_limits));

    // save handle to queue
    handles.push(pi.hProcess);
    if(wait)
    {
        WaitForSingleObject(pi.hProcess, INFINITE);
        CloseHandle(pi.hProcess);
    }
    return retval;

#else
    command = command + " > /dev/null 2>&1";
    HANDLE pid;
    int status;
    switch(pid = fork())
    {
    case -1: /// error
        return false;
    case 0: /// child
    {
        setpgid(0, 0);
        char curdir[1024] = {};
        getcwd(curdir, 1023);
        chdir(runpath.data());
        execlp("sh", "sh", "-c", command.data(), (char*)NULL);
        _exit(127);
    }
    default: /// parent
        if(wait)
            waitpid(pid, &status, 0);
        else
            handles.emplace(pid);
    }
    return true;
#endif // _WIN32

}

void killByHandle()
{
    while(!handles.empty())
    {
        HANDLE hProc = handles.front();
#ifdef _WIN32
        if(hProc != NULL)
        {
            if(TerminateProcess(hProc, 0))
                CloseHandle(hProc);
        }
#else
        if(hProc != 0)
            kill(-hProc, SIGINT); // kill process group
#endif // _WIN32
        handles.pop();
    }
}

bool killProgram(std::string program)
{
#ifdef _WIN32
    HANDLE hShot = CreateToolhelp32Snapshot(TH32CS_SNAPALL, 0), hProcess = 0;
    if(hShot == INVALID_HANDLE_VALUE)
    {
        return false;
    }
    PROCESSENTRY32 pe = {};
    pe.dwSize = sizeof(pe);

    for(BOOL bRet = Process32First(hShot, &pe); bRet; bRet = Process32Next(hShot, &pe))
    {
        if(program.compare(pe.szExeFile) == 0)
        {
            hProcess = OpenProcess(PROCESS_TERMINATE, FALSE, pe.th32ProcessID);
            if(hProcess == NULL)
            {
                CloseHandle(hShot);
                return false;
            }

            DWORD result = WAIT_OBJECT_0;
            while(result == WAIT_OBJECT_0)
            {
                result = WaitForSingleObject(hProcess, 50);
                TerminateProcess(hProcess, 0);
            }
            CloseHandle(hProcess);
        }
    }
    CloseHandle(hShot);
    return true;
#else
    program = "pkill -f " + program;
    system(program.data());
    return true;
#endif
}

