cmake_minimum_required(VERSION 3.20)
project(lunu-builder VERSION 0.1.0 LANGUAGES CXX)

# Standard C++20
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Output directories
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/lib)
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/lib)
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)

# Include directories
include_directories(include)

# Source files
file(GLOB_RECURSE SOURCES "src/*.cpp")

# Executable
add_executable(lunu-builder ${SOURCES})

# Installation (optional)
install(TARGETS lunu-builder DESTINATION bin)
