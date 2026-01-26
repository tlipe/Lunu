#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <windows.h>

// Note: This C stub requires a ZIP library (e.g., miniz) to function.
// It is provided as a reference implementation for future size optimization (targeting <50KB).

#define TEMP_PREFIX "lunu_app_"

void panic(const char* msg) {
    fprintf(stderr, "\n[Lunu Stub] CRITICAL: %s\n", msg);
    fprintf(stderr, "Press Enter to exit...\n");
    getchar();
    exit(1);
}

void extract_payload(const char* exe_path, const char* out_dir) {
    // 1. Open EXE
    // 2. Find ZIP EOCD signature (0x06054b50) from the end
    // 3. Extract all files to out_dir
    // Implementation omitted (requires ~500 lines of C or miniz dependency)
    printf("[Stub] Extracting payload from %s to %s...\n", exe_path, out_dir);
}

int main() {
    char exe_path[MAX_PATH];
    if (GetModuleFileNameA(NULL, exe_path, MAX_PATH) == 0) {
        panic("Failed to get executable path");
    }

    // Create Temp Dir
    char temp_path[MAX_PATH];
    GetTempPathA(MAX_PATH, temp_path);
    
    char unique_dir[MAX_PATH];
    sprintf(unique_dir, "%slunu_%lu", temp_path, GetCurrentProcessId());
    
    if (!CreateDirectoryA(unique_dir, NULL) && GetLastError() != ERROR_ALREADY_EXISTS) {
        panic("Failed to create temp directory");
    }

    // Extract
    extract_payload(exe_path, unique_dir);

    // Locate Lune
    char lune_path[MAX_PATH];
    sprintf(lune_path, "%s\\bin\\lune.exe", unique_dir);
    
    char script_path[MAX_PATH];
    sprintf(script_path, "%s\\src\\main.luau", unique_dir);

    // Run Lune
    STARTUPINFOA si;
    PROCESS_INFORMATION pi;
    ZeroMemory(&si, sizeof(si));
    si.cb = sizeof(si);
    ZeroMemory(&pi, sizeof(pi));

    char cmd_line[MAX_PATH * 2];
    sprintf(cmd_line, "\"%s\" run \"%s\"", lune_path, script_path);

    if (!CreateProcessA(NULL, cmd_line, NULL, NULL, FALSE, 0, NULL, unique_dir, &si, &pi)) {
        panic("Failed to start Lune runtime");
    }

    WaitForSingleObject(pi.hProcess, INFINITE);
    
    DWORD exit_code;
    GetExitCodeProcess(pi.hProcess, &exit_code);

    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);

    // Cleanup
    // RemoveDirectoryA(unique_dir); // Recursive delete needed

    return exit_code;
}
