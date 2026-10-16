// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

// Native crashes (access violations, illegal instructions, aborts from C++) that Rust's panic
// hook never sees. A short report goes to a file chosen at start-up; on Windows a message box
// says so at once, elsewhere the report is shown when Reviera next opens. The handlers only use
// what is safe inside a signal handler: the text is prepared in `reviera_crash_install`.

#include <signal.h>
#include <stdint.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
#else
#include <fcntl.h>
#include <unistd.h>
#endif

static char g_path[4096];
static char g_header[4096];
static volatile sig_atomic_t g_done = 0;

#ifdef _WIN32
static wchar_t g_wpath[4096];
static wchar_t g_message[4096];
#else
static struct sigaction g_old[32];
#endif

// A report has already been written (by the panic hook): a following abort adds nothing.
void reviera_crash_mark(void) { g_done = 1; }

static void hex(char* out, uintptr_t v) {
    static const char digits[] = "0123456789abcdef";
    char tmp[2 * sizeof v];
    int n = 0;
    do {
        tmp[n++] = digits[v & 15];
        v >>= 4;
    } while (v && n < (int)sizeof tmp);
    *out++ = '0';
    *out++ = 'x';
    while (n) *out++ = tmp[--n];
    *out = 0;
}

static void write_report(const char* what, uintptr_t code, uintptr_t address) {
    char line[512];
    char num[32];
    line[0] = 0;
    strcat(line, "Crash: ");
    strcat(line, what);
    strcat(line, " (code ");
    hex(num, code);
    strcat(line, num);
    strcat(line, ", address ");
    hex(num, address);
    strcat(line, num);
    strcat(line, ")\n");
#ifdef _WIN32
    HANDLE f = CreateFileW(g_wpath, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL,
                           NULL);
    if (f != INVALID_HANDLE_VALUE) {
        DWORD written;
        WriteFile(f, g_header, (DWORD)strlen(g_header), &written, NULL);
        WriteFile(f, line, (DWORD)strlen(line), &written, NULL);
        CloseHandle(f);
    }
#else
    int f = open(g_path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (f >= 0) {
        ssize_t r = write(f, g_header, strlen(g_header));
        r = write(f, line, strlen(line));
        (void)r;
        close(f);
    }
#endif
}

#ifdef _WIN32
static const char* exception_name(DWORD code) {
    switch (code) {
    case EXCEPTION_ACCESS_VIOLATION: return "access violation";
    case EXCEPTION_STACK_OVERFLOW: return "stack overflow";
    case EXCEPTION_ILLEGAL_INSTRUCTION: return "illegal instruction";
    case EXCEPTION_INT_DIVIDE_BY_ZERO: return "division by zero";
    case EXCEPTION_IN_PAGE_ERROR: return "page error";
    default: return "unhandled exception";
    }
}

static void tell(void) {
    MessageBoxW(NULL, g_message, L"BIM Reviera", MB_OK | MB_ICONERROR | MB_TASKMODAL);
}

static LONG WINAPI on_exception(EXCEPTION_POINTERS* e) {
    if (!g_done) {
        g_done = 1;
        DWORD code = e->ExceptionRecord->ExceptionCode;
        write_report(exception_name(code), code,
                     (uintptr_t)e->ExceptionRecord->ExceptionAddress);
        tell();
    }
    return EXCEPTION_EXECUTE_HANDLER;
}

static void on_abort(int sig) {
    if (!g_done) {
        g_done = 1;
        write_report("abort", (uintptr_t)sig, 0);
        tell();
    }
    signal(sig, SIG_DFL);
    raise(sig);
}
#else
static const char* signal_name(int sig) {
    switch (sig) {
    case SIGSEGV: return "segmentation fault";
    case SIGBUS: return "bus error";
    case SIGILL: return "illegal instruction";
    case SIGFPE: return "arithmetic error";
    case SIGABRT: return "abort";
    default: return "signal";
    }
}

static void on_signal(int sig, siginfo_t* info, void* context) {
    (void)context;
    if (!g_done) {
        g_done = 1;
        write_report(signal_name(sig), (uintptr_t)sig, (uintptr_t)(info ? info->si_addr : 0));
    }
    // The previous handler (Rust's stack-overflow check, or the default) takes over: a fault
    // happens again when the instruction re-runs; abort() raises again itself.
    sigaction(sig, &g_old[sig], NULL);
}
#endif

// `path` is where a report goes; `header` its first lines; `message` the text shown at once
// (Windows). All UTF-8.
void reviera_crash_install(const char* path, const char* header, const char* message) {
    strncpy(g_path, path, sizeof g_path - 1);
    strncpy(g_header, header, sizeof g_header - 1);
#ifdef _WIN32
    MultiByteToWideChar(CP_UTF8, 0, path, -1, g_wpath, (int)(sizeof g_wpath / sizeof *g_wpath));
    MultiByteToWideChar(CP_UTF8, 0, message, -1, g_message,
                        (int)(sizeof g_message / sizeof *g_message));
    SetUnhandledExceptionFilter(on_exception);
    signal(SIGABRT, on_abort);
#else
    (void)message;
    int signals[] = {SIGSEGV, SIGBUS, SIGILL, SIGFPE, SIGABRT};
    for (unsigned i = 0; i < sizeof signals / sizeof *signals; i++) {
        struct sigaction sa;
        memset(&sa, 0, sizeof sa);
        sa.sa_sigaction = on_signal;
        sa.sa_flags = SA_SIGINFO | SA_ONSTACK;
        sigemptyset(&sa.sa_mask);
        sigaction(signals[i], &sa, &g_old[signals[i]]);
    }
#endif
}
