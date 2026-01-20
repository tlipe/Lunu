#include <iostream>
#include <string>
#include <vector>

using namespace std;

void print_usage() {
    cout << "Lunu Builder v0.1.0" << endl;
    cout << "Usage: lunu-builder <command> [options]" << endl;
    cout << "Commands:" << endl;
    cout << "  build <file.luau>   Compile a Luau file (placeholder)" << endl;
    cout << "  version             Show version" << endl;
}

int main(int argc, char* argv[]) {
    if (argc < 2) {
        print_usage();
        return 1;
    }

    string command = argv[1];

    if (command == "version") {
        cout << "Lunu Builder v0.1.0" << endl;
    } else if (command == "build") {
        if (argc < 3) {
            cerr << "Error: Missing file argument." << endl;
            return 1;
        }
        string filename = argv[2];
        cout << "[Builder] Processing " << filename << "..." << endl;

        string output_exe = filename.substr(0, filename.find_last_of(".")) + ".exe";
        
        cout << "[Builder] Target: Standalone Executable" << endl;
        cout << "[Builder] Output: ./" << output_exe << " (In current directory)" << endl;
        
        cout << "[Builder] TODO: Implement Luau compilation & packing logic here." << endl;
        // Bota aqui tua logica do build, segue os passos:
        // 1. Compilar o arquivo .luau para bytecode.
        // 2. Empacotar o bytecode com o runtime do Lunu.
        // 3. Salvar o executável no diretório atual.
        

        
        //---------------------------------------------
    } else {
        cerr << "Unknown command: " << command << endl;
        print_usage();
        return 1;
    }

    return 0;
}
