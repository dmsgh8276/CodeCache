package main

type Server struct {
	addr string
}

func (s *Server) Handle(path string) string {
	return s.addr + path
}
