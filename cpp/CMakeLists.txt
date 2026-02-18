cmake_minimum_required(VERSION 3.10)
project(turbo_engine)

set(CMAKE_CXX_STANDARD 11)
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -O3 -fPIC")

find_package(CURL REQUIRED)
add_library(turbo_engine SHARED turbo_engine.cpp)
target_link_libraries(turbo_engine PRIVATE CURL::libcurl)

target_include_directories(turbo_engine PUBLIC include)