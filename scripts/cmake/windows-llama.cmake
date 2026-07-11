if(WIN32 AND PROJECT_NAME STREQUAL "llama.cpp")
    # Keep llama.cpp's nested Vulkan shader tool below MSVC's path limit while
    # retaining all native build output in Cargo's target directory.
    set_property(DIRECTORY PROPERTY EP_BASE "${CMAKE_BINARY_DIR}/ep")
endif()
